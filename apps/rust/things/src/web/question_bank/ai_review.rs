//! Question Bank AI review: start, follow-up, retry, delete, SSE snapshot and
//! timeline assembly for the `analysis` / `grading` scopes.
use super::ai_review_api::*;
use super::*;
use hyz_things::domain::question_bank::AiReviewState;

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionAiReviewProps {
    pub csrf: String,
    pub question_id: String,
    pub question_content: Html,
    pub question_scroll: NodeRef,
    pub on_question_scroll: Callback<Event>,
    #[prop_or_default]
    pub initial_review: Option<Rc<RefCell<Option<AiReviewState>>>>,
    #[prop_or_default]
    pub delete_signal: u32,
    #[prop_or_default]
    pub on_delete_availability: Callback<bool>,
    #[prop_or("analysis")]
    pub mode: &'static str,
    #[prop_or_default]
    pub draft_namespace: Option<&'static str>,
}

/// The component is mounted only while the AI tab is visible; the native
/// backend owns its turns and never cancels on unmount or question navigation.
#[function_component(QuestionAiReview)]
pub(crate) fn question_ai_review(props: &QuestionAiReviewProps) -> Html {
    let initial = use_mut_ref(|| {
        props
            .initial_review
            .as_ref()
            .and_then(|value| value.borrow_mut().take())
            .filter(|value| value.question_id == props.question_id && props.mode == "analysis")
    });
    let state = use_state(|| initial.borrow().clone());
    let history_generation = use_state(|| 0u64);
    let history_has_runs = use_state(|| None::<bool>);
    {
        let state_has_content = state
            .as_ref()
            .as_ref()
            .is_some_and(|value| value.status != "idle" || !value.messages.is_empty());
        let can_delete = history_has_runs.unwrap_or(false) || state_has_content;
        let availability_known = history_has_runs.is_some() || state_has_content;
        let notify = props.on_delete_availability.clone();
        use_effect_with(
            (availability_known, can_delete),
            move |(known, available)| {
                if *known {
                    notify.emit(*available);
                }
                || ()
            },
        );
    }
    let diagram_root = use_node_ref();
    let failure = use_state(|| None::<String>);
    let submitting = use_state(|| false);
    let scope = if props.mode == "grade" {
        "grading"
    } else {
        "analysis"
    };
    let draft_scope = props.draft_namespace.map_or_else(
        || scope.to_owned(),
        |namespace| format!("{namespace}-{scope}"),
    );
    let follow_key = ai_draft_key(&props.question_id, &draft_scope, "follow-up");
    let start_key = ai_draft_key(&props.question_id, &draft_scope, "start");
    let follow_up = use_state(|| ai_draft_read(&follow_key));
    let start_instruction = use_state(|| ai_draft_read(&start_key));
    let request_key = ai_draft_key(&props.question_id, &draft_scope, "request-id");
    let request_id = use_state(|| {
        let value = ai_draft_read(&request_key);
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    });
    let request_info = use_state(|| None::<AiRequestInfo>);
    let request_check = use_state(|| 0u64);
    let request_busy = use_mut_ref(|| false);
    let request_error = use_state(|| None::<String>);
    let request_not_found = use_state(|| false);
    let retry_open = use_state(|| false);
    let confirm_uncertain = use_state(|| false);
    let confirm_new_thread = use_state(|| false);
    let retry_expired = use_state(|| false);
    let retry_images = use_state(Vec::<PendingImage>::new);
    let retry_uploading = use_state(|| false);
    let pending_message = use_state(|| None::<String>);
    let messages_ref = props.question_scroll.clone();
    let follow_bottom = use_mut_ref(|| true);
    let new_content = use_state(|| false);
    let start_images = use_state(Vec::<PendingImage>::new);
    let follow_images = use_state(Vec::<PendingImage>::new);
    let uploading_start = use_state(|| false);
    let uploading_follow = use_state(|| false);
    let start_input_ref = use_node_ref();
    let follow_input_ref = use_node_ref();
    {
        let start_ref = start_input_ref.clone();
        let follow_ref = follow_input_ref.clone();
        let key = (
            (*start_instruction).clone(),
            (*follow_up).clone(),
            start_images.len(),
            follow_images.len(),
            state.as_ref().as_ref().map(|value| value.status.clone()),
        );
        use_effect_with(key, move |_| {
            for node in [&start_ref, &follow_ref] {
                if let Some(input) = node.cast::<web_sys::HtmlTextAreaElement>() {
                    let _ = input.style().set_property("height", "auto");
                    let _ = input.style().set_property(
                        "height",
                        &format!("{}px", input.scroll_height().clamp(24, 160)),
                    );
                }
            }
            || ()
        });
    }
    let deleting = use_state(|| false);
    let delete_confirmation_required = use_state(|| false);
    let confirm_delete_uncertain = use_state(|| false);
    let delete_error = use_state(|| None::<String>);
    {
        let previous = use_mut_ref(|| props.delete_signal);
        let deleting = deleting.clone();
        let delete_confirmation_required = delete_confirmation_required.clone();
        let confirm_delete_uncertain = confirm_delete_uncertain.clone();
        let delete_error = delete_error.clone();
        use_effect_with(props.delete_signal, move |signal| {
            if *signal != *previous.borrow() {
                delete_confirmation_required.set(false);
                confirm_delete_uncertain.set(false);
                delete_error.set(None);
                deleting.set(true);
            }
            *previous.borrow_mut() = *signal;
            || ()
        });
    }
    let revision = use_mut_ref(|| 0u64);
    let endpoint = format!("{ROOT}/practice/questions/{}/ai", props.question_id);
    {
        let state = state.clone();
        let failure = failure.clone();
        let endpoint = endpoint.clone();
        let scope = scope;
        let revision = revision.clone();
        let initial = initial.clone();
        let key = props.question_id.clone();
        use_effect_with(key, move |_| {
            if initial.borrow_mut().take().is_some() {
                return Box::new(|| ()) as Box<dyn FnOnce()>;
            }
            state.set(None);
            failure.set(None);
            let alive = std::rc::Rc::new(std::cell::Cell::new(true));
            let request_alive = alive.clone();
            let request_revision = *revision.borrow();
            let endpoint = ai_scope_url(&endpoint, "", scope, None);
            spawn_local(async move {
                let response = fetch_json::<AiReviewState>(&endpoint, "AI 复盘记录").await;
                if request_alive.get() && *revision.borrow() == request_revision {
                    match response {
                        Ok(value) => state.set(Some(value)),
                        Err(error) => failure.set(Some(error)),
                    }
                }
            });
            Box::new(move || alive.set(false)) as Box<dyn FnOnce()>
        });
    }
    let (stream, disconnected, reconnect_stream) = super::ai_review_stream::use_ai_review_stream(
        props.question_id.clone(),
        endpoint.clone(),
        scope,
        state.clone(),
        revision.clone(),
    );
    {
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_not_found = request_not_found.clone();
        let state = state.clone();
        let endpoint = endpoint.clone();
        let scope = scope;
        let key = (
            (*request_id).clone(),
            (*stream).as_ref().map(|value| value.status.clone()),
            *request_check,
            *submitting,
        );
        use_effect_with(key, move |(id, _, _, busy)| {
            let alive = Rc::new(Cell::new(true));
            if let Some(id) = id.clone().filter(|_| !*busy) {
                let alive = alive.clone();
                let request_info = request_info.clone();
                let request_error = request_error.clone();
                let request_not_found = request_not_found.clone();
                let state = state.clone();
                spawn_local(async move {
                    match fetch_json_typed::<AiRequestInfo>(
                        &ai_scope_url(&endpoint, &format!("/requests/{id}"), scope, None),
                        "AI 请求状态",
                    )
                    .await
                    {
                        Ok(info) if alive.get() => {
                            request_not_found.set(false);
                            request_error.set(None);
                            request_info.set(Some(info));
                        }
                        Err(error)
                            if alive.get()
                                && error.is_code("question_bank_ai_request_not_found") =>
                        {
                            // Do not discard a missing ID on the strength of one 404: a
                            // server task may still be active after a lost response.
                            let review = fetch_json::<AiReviewState>(
                                &ai_scope_url(&endpoint, "", scope, None),
                                "AI 复盘记录",
                            )
                            .await;
                            if alive.get() {
                                request_info.set(None);
                                match review {
                                    Ok(value) => {
                                        state.set(Some(value));
                                        request_not_found.set(true);
                                        request_error.set(Some("服务端不存在这条 AI 请求记录，已重新核对题目状态。若决定放弃追踪原请求，可手动清除失效标识；不会自动重发。".into()));
                                    }
                                    Err(review_error) => {
                                        request_not_found.set(false);
                                        request_error.set(Some(format!("AI 请求不存在，但无法核对题目状态：{review_error}。请重新查询。")));
                                    }
                                }
                            }
                        }
                        Err(error) if alive.get() => {
                            request_not_found.set(false);
                            request_info.set(None);
                            request_error.set(Some(format!(
                                "请求结果尚未确认：{error}。可再次查询，不会自动重发。"
                            )));
                        }
                        _ => (),
                    }
                });
            }
            move || alive.set(false)
        });
    }
    {
        let diagram_root = diagram_root.clone();
        let key = (
            props.question_id.clone(),
            (*state).clone(),
            (*stream).clone(),
        );
        use_effect_with(key, move |_| {
            if let Some(root) = diagram_root.cast::<HtmlElement>() {
                hydrate_math_element(&root);
                render_markdown_diagrams(&root);
            }
            || ()
        });
    }
    {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        let key = (
            (*state).clone(),
            (*pending_message).clone(),
            (*stream).clone(),
        );
        use_effect_with(key, move |_| {
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                if element.has_attribute("data-review-scroll-restoring") {
                    // Returning to a review question restores its saved position
                    // after AI history arrives; never override it with chat autoscroll.
                    *follow_bottom.borrow_mut() = false;
                    new_content.set(false);
                } else if *follow_bottom.borrow() {
                    element.set_scroll_top(element.scroll_height());
                    new_content.set(false);
                } else {
                    new_content.set(true);
                }
            }
            || ()
        });
    }
    let on_messages_scroll = {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        let on_question_scroll = props.on_question_scroll.clone();
        Callback::from(move |event: Event| {
            on_question_scroll.emit(event);
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                let near_bottom =
                    element.scroll_height() - element.client_height() - element.scroll_top() <= 48;
                *follow_bottom.borrow_mut() = near_bottom;
                if near_bottom {
                    new_content.set(false);
                }
            }
        })
    };
    let scroll_to_bottom = {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        Callback::from(move |_: MouseEvent| {
            *follow_bottom.borrow_mut() = true;
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                element.set_scroll_top(element.scroll_height());
            }
            new_content.set(false);
        })
    };
    let start_review = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let endpoint = endpoint.clone();
        let scope = scope;
        let csrf = props.csrf.clone();
        let start_instruction = start_instruction.clone();
        let start_images = start_images.clone();
        let uploading_start = uploading_start.clone();
        let start_key = start_key.clone();
        Callback::from(move |_: ()| {
            if *submitting || *uploading_start {
                return;
            }
            if request_id.is_some()
                && request_info.as_ref().as_ref().is_none_or(|info| {
                    Some(&info.request_id) != (*request_id).as_ref()
                        || info.remote_uncertain
                        || matches!(info.status.as_str(), "unknown" | "running")
                })
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let previous_id = (*request_id).clone();
            let previous_info = (*request_info).clone();
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id.clone()));
            request_info.set(None);
            request_error.set(None);
            submitting.set(true);
            failure.set(None);
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let endpoint = ai_scope_url(&endpoint, "", scope, None);
            let csrf = csrf.clone();
            let draft = (*start_instruction).clone();
            let instruction = draft.trim().to_owned();
            let start_key = start_key.clone();
            let start_instruction = start_instruction.clone();
            let start_images = start_images.clone();
            let attachments = start_images.iter().map(|image| image.id.clone()).collect();
            spawn_local(async move {
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &AiReviewStartRequest {
                        request_id: next_id,
                        instruction,
                        attachments,
                    },
                    "开始 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => {
                        request_info.set(Some(result.request));
                        state.set(Some(result.state));
                        if ai_draft_read(&start_key) == draft {
                            ai_draft_write(&start_key, "");
                            start_instruction.set(String::new());
                        }
                        start_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, previous_id.as_deref().unwrap_or(""));
                            request_id.set(previous_id);
                            request_info.set(previous_info);
                            if error.is_code("question_bank_ai_attachment_expired") {
                                start_images.set(Vec::new());
                            }
                        }
                        failure.set(Some(error.to_string()));
                    }
                }
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let on_start = {
        let start = start_review.clone();
        Callback::from(move |_: MouseEvent| start.emit(()))
    };
    let on_start_key = {
        let start = start_review.clone();
        Callback::from(move |event: KeyboardEvent| {
            if !event.is_composing()
                && event.key() == "Enter"
                && (event.ctrl_key() || event.meta_key())
            {
                event.prevent_default();
                start.emit(());
            }
        })
    };
    let on_stop = {
        let state = state.clone();
        let request_check = request_check.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let endpoint = ai_scope_url(&endpoint, "/stop", scope, None);
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *submitting {
                return;
            }
            submitting.set(true);
            let state = state.clone();
            let request_check = request_check.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response::<_, AiReviewState>(
                    &endpoint,
                    &csrf,
                    &EmptyRequest {},
                    "停止 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => state.set(Some(result)),
                    Err(error) => failure.set(Some(error)),
                }
                request_check.set(*request_check + 1);
                reconnect_stream.emit(());
                submitting.set(false);
            });
        })
    };
    let send_follow_up = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let follow_up = follow_up.clone();
        let follow_images = follow_images.clone();
        let uploading_follow = uploading_follow.clone();
        let current_state = state.clone();
        let stream = stream.clone();
        let pending_message = pending_message.clone();
        let follow_bottom = follow_bottom.clone();
        let follow_key = follow_key.clone();
        let endpoint = ai_scope_url(&endpoint, "/messages", scope, None);
        let csrf = props.csrf.clone();
        let grade_mode = props.mode == "grade";
        Callback::from(move |_: ()| {
            if *submitting
                || *uploading_follow
                || current_state
                    .as_ref()
                    .as_ref()
                    .is_some_and(|value| value.status == "running")
                || stream
                    .as_ref()
                    .as_ref()
                    .is_some_and(|value| value.status == "running")
            {
                return;
            }
            let draft = (*follow_up).clone();
            let message = if grade_mode {
                format!("【申论批改请求或追问】\n如果以下内容是一份新答案，请按当前题目给出完整批改；如果是针对之前报告的问题，请直接解答，不把问题误认为答卷。用户确认此前的手写转写无误时，可以继续完成评分。\n{}", draft.trim())
            } else {
                draft.trim().to_owned()
            };
            let message = message.trim().to_owned();
            if (message.is_empty() && follow_images.is_empty())
                || message.chars().count() > (if grade_mode { 50_256 } else { 2000 })
            {
                return;
            }
            if request_id.is_some()
                && request_info.as_ref().as_ref().is_none_or(|info| {
                    Some(&info.request_id) != (*request_id).as_ref()
                        || info.remote_uncertain
                        || matches!(info.status.as_str(), "unknown" | "running")
                })
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let previous_id = (*request_id).clone();
            let previous_info = (*request_info).clone();
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id.clone()));
            request_info.set(None);
            request_error.set(None);
            submitting.set(true);
            failure.set(None);
            *follow_bottom.borrow_mut() = true;
            pending_message.set(Some(if message.is_empty() {
                "图片追问".into()
            } else {
                message.clone()
            }));
            let pending_message = pending_message.clone();
            let follow_key = follow_key.clone();
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let follow_up = follow_up.clone();
            let follow_images = follow_images.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            let attachments = follow_images.iter().map(|image| image.id.clone()).collect();
            spawn_local(async move {
                let payload = AiReviewFollowUpRequest {
                    request_id: next_id,
                    message,
                    attachments,
                };
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &payload,
                    "AI 追问",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => {
                        request_info.set(Some(result.request));
                        state.set(Some(result.state));
                        if ai_draft_read(&follow_key) == draft {
                            ai_draft_write(&follow_key, "");
                            follow_up.set(String::new());
                        }
                        follow_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, previous_id.as_deref().unwrap_or(""));
                            request_id.set(previous_id);
                            request_info.set(previous_info);
                            if error.is_code("question_bank_ai_attachment_expired") {
                                follow_images.set(Vec::new());
                            }
                        }
                        failure.set(Some(format!("{error}。请先查询请求状态。")));
                    }
                }
                pending_message.set(None);
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let on_follow_up = {
        let send = send_follow_up.clone();
        Callback::from(move |_: MouseEvent| send.emit(()))
    };
    let on_follow_up_key = {
        let send = send_follow_up.clone();
        Callback::from(move |event: KeyboardEvent| {
            if !event.is_composing()
                && event.key() == "Enter"
                && (event.ctrl_key() || event.meta_key())
            {
                event.prevent_default();
                send.emit(());
            }
        })
    };
    let on_delete = {
        let state = state.clone();
        let history_generation = history_generation.clone();
        let history_has_runs = history_has_runs.clone();
        let request_id = request_id.clone();
        let request_info = request_info.clone();
        let request_not_found = request_not_found.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let submitting = submitting.clone();
        let deleting = deleting.clone();
        let delete_confirmation_required = delete_confirmation_required.clone();
        let confirm_delete_uncertain = confirm_delete_uncertain.clone();
        let delete_error = delete_error.clone();
        let endpoint = ai_scope_url(&endpoint, "", scope, None);
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *submitting || (*delete_confirmation_required && !*confirm_delete_uncertain) {
                return;
            }
            submitting.set(true);
            delete_error.set(None);
            let state = state.clone();
            let history_generation = history_generation.clone();
            let history_has_runs = history_has_runs.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_not_found = request_not_found.clone();
            let request_error = request_error.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let submitting = submitting.clone();
            let deleting = deleting.clone();
            let delete_confirmation_required = delete_confirmation_required.clone();
            let confirm_delete_uncertain = confirm_delete_uncertain.clone();
            let delete_error = delete_error.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            let confirmed = *confirm_delete_uncertain;
            spawn_local(async move {
                let result = delete_json_body_response_typed::<_, AiReviewState>(
                    &endpoint,
                    &csrf,
                    &AiDeleteRequest {
                        confirm_uncertain: confirmed,
                    },
                    "删除 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(value) => {
                        state.set(Some(value));
                        history_has_runs.set(Some(false));
                        history_generation.set(history_generation.wrapping_add(1));
                        ai_draft_write(&request_key, "");
                        request_id.set(None);
                        request_info.set(None);
                        request_not_found.set(false);
                        request_error.set(None);
                        delete_confirmation_required.set(false);
                        confirm_delete_uncertain.set(false);
                        deleting.set(false);
                    }
                    Err(error) if error.is_code("question_bank_ai_confirmation_required") => {
                        delete_confirmation_required.set(true);
                        confirm_delete_uncertain.set(false);
                        delete_error.set(None);
                    }
                    Err(error) => delete_error.set(Some(error.to_string())),
                }
                reconnect_stream.emit(());
                submitting.set(false);
            });
        })
    };
    let on_retry = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_key = request_key.clone();
        let request_error = request_error.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let retry_open = retry_open.clone();
        let retry_expired = retry_expired.clone();
        let retry_images = retry_images.clone();
        let retry_uploading = retry_uploading.clone();
        let confirm_uncertain = confirm_uncertain.clone();
        let confirm_new_thread = confirm_new_thread.clone();
        let endpoint = endpoint.clone();
        let csrf = props.csrf.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        Callback::from(move |_: MouseEvent| {
            let Some(source) = (*request_info).clone() else {
                return;
            };
            if *submitting
                || *retry_uploading
                || Some(&source.request_id) != (*request_id).as_ref()
                || !matches!(source.status.as_str(), "failed" | "stopped" | "unknown")
                || (source.remote_uncertain && !*confirm_uncertain)
                || (source
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("无法恢复原 Codex Thread"))
                    && !*confirm_new_thread)
                || (*retry_expired && retry_images.is_empty())
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let payload = AiRetryRequest {
                request_id: next_id.clone(),
                confirm_uncertain: *confirm_uncertain,
                confirm_new_thread: *confirm_new_thread,
                replacement_attachments: if *retry_expired {
                    Some(retry_images.iter().map(|image| image.id.clone()).collect())
                } else {
                    None
                },
            };
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id));
            request_info.set(None);
            request_error.set(None);
            failure.set(None);
            submitting.set(true);
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let retry_open = retry_open.clone();
            let retry_expired = retry_expired.clone();
            let retry_images = retry_images.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let endpoint = ai_scope_url(
                &endpoint,
                &format!("/requests/{}/retry", source.request_id),
                scope,
                None,
            );
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &payload,
                    "重试 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(receipt) => {
                        request_info.set(Some(receipt.request));
                        state.set(Some(receipt.state));
                        retry_open.set(false);
                        retry_expired.set(false);
                        retry_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, &source.request_id);
                            request_id.set(Some(source.request_id.clone()));
                            request_info.set(Some(source));
                            if error.is_code("question_bank_ai_attachment_expired") {
                                retry_expired.set(true);
                            }
                        }
                        failure.set(Some(error.to_string()));
                    }
                }
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let current: Option<AiReviewState> = (*state).clone();
    let running = stream
        .as_ref()
        .as_ref()
        .is_some_and(|value| value.status == "running")
        || current
            .as_ref()
            .is_some_and(|value| value.status == "running")
        || request_info
            .as_ref()
            .as_ref()
            .is_some_and(|info| info.status == "running");
    let has_answer = current
        .as_ref()
        .is_some_and(|value| value.messages.iter().any(|msg| msg.role == "assistant"));
    let request_unresolved = request_id.is_some()
        && request_info
            .as_ref()
            .as_ref()
            .is_none_or(|info| Some(&info.request_id) != (*request_id).as_ref());
    let request_uncertain = request_info
        .as_ref()
        .as_ref()
        .is_some_and(|info| info.remote_uncertain || info.status == "unknown");
    let can_start = !running
        && current
            .as_ref()
            .is_some_and(|value| value.messages.is_empty() && value.status == "idle");
    let can_retry = !running
        && request_info.as_ref().as_ref().is_some_and(|info| {
            Some(&info.request_id) == (*request_id).as_ref()
                && matches!(info.status.as_str(), "failed" | "stopped" | "unknown")
        });
    html! {
        <section class="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="question-bank-ai-review" ref={diagram_root}>
            <div class="min-h-0 flex-1 space-y-3 overflow-y-auto overscroll-contain touch-pan-y p-4" ref={messages_ref} onscroll={on_messages_scroll} data-testid="question-bank-ai-messages" data-ink-pane="answer" role="log" aria-label="AI 对话记录" aria-live="polite">
            <div class="space-y-4" data-testid="question-bank-ai-question-content">{props.question_content.clone()}</div>
            <div class="flex items-center justify-between gap-2">
                <strong>{if props.mode=="grade" {"AI 批改与追问"} else {"AI 解析与追问"}}</strong>
                <span class="text-xs text-base-content/60" data-testid="question-bank-ai-status">
                    {if running { "正在生成" } else { match stream.as_ref().as_ref().map(|v| v.status.as_str()).or_else(|| current.as_ref().map(|v| v.status.as_str())) {
                        Some("complete") => "已完成",
                        Some("stopped") => "已停止",
                        Some("failed") => "生成失败",
                        Some(_) => "未生成",
                        None => "正在加载",
                    } }}
                </span>
            </div>
            if *disconnected {
                <p class="text-xs text-warning" role="status" data-testid="question-bank-ai-disconnected">{"连接中断，正在重新连接…"}</p>
            }
            if let Some(error) = failure.as_ref() {
                <p class="text-sm text-error" role="alert">{error.clone()}</p>
            }
            <AiReviewRecovery
                request_present={request_id.is_some()}
                request_unresolved={request_unresolved}
                request_error={(*request_error).clone()}
                request_not_found={*request_not_found}
                running={running}
                disconnected={*disconnected}
                submitting={*submitting}
                can_retry={can_retry}
                info={(*request_info).clone()}
                retry_open={*retry_open}
                confirm_uncertain={*confirm_uncertain}
                confirm_new_thread={*confirm_new_thread}
                retry_expired={*retry_expired}
                retry_images={retry_images.clone()}
                retry_uploading={retry_uploading.clone()}
                endpoint={endpoint.clone()}
                csrf={props.csrf.clone()}
                scope={scope}
                on_request_check={Callback::from({let request_check=request_check.clone(); move |_| request_check.set(*request_check + 1)})}
                on_discard={Callback::from({
                    let request_key=request_key.clone();
                    let request_id=request_id.clone();
                    let request_info=request_info.clone();
                    let request_not_found=request_not_found.clone();
                    let request_error=request_error.clone();
                    let failure=failure.clone();
                    move |_| {
                        ai_draft_write(&request_key, "");
                        request_id.set(None);
                        request_info.set(None);
                        request_not_found.set(false);
                        request_error.set(None);
                        failure.set(None);
                    }
                })}
                on_open_retry={Callback::from({let retry_open=retry_open.clone(); let confirm_uncertain=confirm_uncertain.clone(); let confirm_new_thread=confirm_new_thread.clone(); move |_| { confirm_uncertain.set(false); confirm_new_thread.set(false); retry_open.set(true); }})}
                on_close_retry={Callback::from({let retry_open=retry_open.clone(); move |_| retry_open.set(false)})}
                on_retry_uncertain={Callback::from({let confirm_uncertain=confirm_uncertain.clone(); move |value: bool| confirm_uncertain.set(value)})}
                on_retry_new_thread={Callback::from({let confirm_new_thread=confirm_new_thread.clone(); move |value: bool| confirm_new_thread.set(value)})}
                on_retry={on_retry.clone()}/>
            if let Some(value) = current.as_ref() {
                if let Some(error) = &value.error { <p class="text-sm text-error" role="alert">{error.clone()}</p> }
            }
            <super::ai_review_timeline::AiReviewActivity
                key={format!("{}:{}", props.question_id, *history_generation)}
                refresh_key={format!("{:?}", current.as_ref().map(|value| {
                    (&value.status, value.messages.iter().rev().find(|message| message.role == "assistant").map(|message| message.id))
                }))}
                endpoint={endpoint.clone()}
                scope={scope}
                stream={if current.as_ref().is_some_and(|value| value.status == "idle") {None} else {(*stream).clone()}}
                messages={current.as_ref().map(|value| value.messages.clone()).unwrap_or_default()}
                on_history_availability={Callback::from({
                    let history_has_runs = history_has_runs.clone();
                    move |available| history_has_runs.set(Some(available))
                })}/>
            if let Some(active) = stream.as_ref().as_ref() {
                if active.status == "running" && !active.text.is_empty() {
                    <div class="py-2" data-testid="question-bank-ai-stream-text">
                        <div class="mb-2 text-xs font-semibold text-base-content/60">{"AI 正在回复"}</div>
                        if streaming_markdown_complete(&active.text) {
                            <div class="study-markdown text-base leading-7" data-ai-diagram-source="true">{render_question_bank_markdown(&active.text)}</div>
                        } else {
                            <div class="whitespace-pre-wrap break-words text-base leading-7">{active.text.clone()}</div>
                        }
                    </div>
                }
            }
            if let Some(message) = pending_message.as_ref() {
                <div class="rounded-xl bg-primary/10 p-3" data-testid="question-bank-ai-pending">
                    <div class="mb-2 text-xs font-semibold text-base-content/60">{"你的追问 · 发送中"}</div>
                    <div class="study-markdown text-base leading-7">{render_question_bank_markdown(message)}</div>
                </div>
            }
            <AiReviewDeleteDialog
                visible={*deleting && current.is_some()}
                confirmation_required={*delete_confirmation_required}
                confirm_uncertain={*confirm_delete_uncertain}
                error={(*delete_error).clone()}
                submitting={*submitting}
                on_cancel={Callback::from({let deleting=deleting.clone(); move |_|deleting.set(false)})}
                on_confirm={on_delete.clone()}
                on_confirm_uncertain={Callback::from({let confirm_delete_uncertain=confirm_delete_uncertain.clone(); move |value: bool| confirm_delete_uncertain.set(value)})}/>
            if can_start {
                <p class="text-sm text-base-content/60">{if props.mode=="grade" {
                    "粘贴作答或上传手写图片。AI 先辨认原图并确认不清楚的文字，再根据原题给出 Markdown 批改报告。"
                } else {
                    "AI 根据原题、相关给定材料和你的要求进行解析，来源参考答案仅作参考。"
                }}</p>
            }
            </div>
            <div class="relative shrink-0 bg-base-100 px-2 pb-3 pt-2" data-testid="question-bank-ai-composer-area">
            if *new_content {
                <button type="button" class="btn btn-circle btn-sm absolute bottom-full left-1/2 z-10 -translate-x-1/2 border border-base-content/15 bg-base-100 shadow-md" aria-label="有新内容，回到底部" data-testid="question-bank-ai-scroll-bottom" onclick={scroll_to_bottom}>{"↓"}</button>
            }
            if can_start {
                <div class="space-y-1">
                    <label class="sr-only" for="question-ai-instruction">{"可选要求"}</label>
                    {ai_composer(AiComposer {
                        id: "question-ai-instruction", input_ref: start_input_ref.clone(),
                        compose_testid: "question-bank-ai-requirements-compose",
                        input_testid: "question-bank-ai-instruction",
                        button_testid: "question-bank-ai-start",
                        value: start_instruction.as_str(), max_length: if props.mode=="grade" {50_000} else {2000}, read_only: false,
                        placeholder: if props.mode=="grade" {"粘贴申论作答，或上传手写图片…"} else {"输入解析要求（可选）…"},
                        button_label: if props.mode=="grade" {"发送批改"} else {"开始 AI 解析"}, is_stop: false,
                        disabled: *submitting || *uploading_start || request_unresolved || request_uncertain || (props.mode=="grade" && start_instruction.trim().is_empty() && start_images.is_empty()),
                        endpoint: ai_scope_url(&endpoint, "/attachments", scope, None), csrf: props.csrf.clone(),
                        attachments: start_images.clone(), uploading: uploading_start.clone(),
                        picker_disabled: *submitting, picker_testid: "question-bank-ai-start-images",
                        oninput: Callback::from({
                            let start_instruction=start_instruction.clone();
                            let start_key=start_key.clone();
                            move |event: InputEvent| {
                                let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
                                ai_draft_write(&start_key, &input.value());
                                start_instruction.set(input.value());
                            }
                        }),
                        onkeydown: on_start_key.clone(),
                        onclick: on_start.clone(),
                    })}
                </div>
            }
            if has_answer || running {
                <div class="space-y-2">
                    <label class="sr-only" for="question-ai-follow-up">{"继续追问"}</label>
                    {ai_composer(AiComposer {
                        id: "question-ai-follow-up", input_ref: follow_input_ref.clone(),
                        compose_testid: "question-bank-ai-compose",
                        input_testid: "question-bank-ai-input",
                        button_testid: if running {"question-bank-ai-stop"} else {"question-bank-ai-send"},
                        value: follow_up.as_str(), max_length: if props.mode=="grade" {50_000} else {2000}, read_only: false,
                        placeholder: if props.mode=="grade" {"追问扣分原因，或发送修改后的答案…"} else {"输入消息…"},
                        button_label: if running {"停止生成"} else {"发送追问"}, is_stop: running,
                        disabled: *submitting || (!running && (*uploading_follow || request_unresolved || request_uncertain || (follow_up.trim().is_empty() && follow_images.is_empty()))),
                        endpoint: ai_scope_url(&endpoint, "/attachments", scope, None), csrf: props.csrf.clone(),
                        attachments: follow_images.clone(), uploading: uploading_follow.clone(),
                        picker_disabled: *submitting, picker_testid: "question-bank-ai-follow-up-images",
                        oninput: Callback::from({
                            let follow_up=follow_up.clone();
                            let follow_key=follow_key.clone();
                            move |event: InputEvent| {
                                let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
                                ai_draft_write(&follow_key, &input.value());
                                follow_up.set(input.value());
                            }
                        }),
                        onkeydown: on_follow_up_key.clone(),
                        onclick: if running {on_stop.clone()} else {on_follow_up.clone()},
                    })}
                </div>
            }
            </div>
        </section>
    }
}
