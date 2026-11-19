//! Request-status, retry and delete recovery UI for the Question Bank AI review.
//! The owning review component keeps the request state machine; this module only
//! renders the fixed, typed recovery actions.
use super::*;

#[derive(Properties, PartialEq)]
pub(super) struct AiReviewRecoveryProps {
    pub(super) request_present: bool,
    pub(super) request_unresolved: bool,
    pub(super) request_error: Option<String>,
    pub(super) request_not_found: bool,
    pub(super) running: bool,
    pub(super) disconnected: bool,
    pub(super) submitting: bool,
    pub(super) can_retry: bool,
    pub(super) info: Option<AiRequestInfo>,
    pub(super) retry_open: bool,
    pub(super) confirm_uncertain: bool,
    pub(super) confirm_new_thread: bool,
    pub(super) retry_expired: bool,
    pub(super) retry_images: UseStateHandle<Vec<PendingImage>>,
    pub(super) retry_uploading: UseStateHandle<bool>,
    pub(super) endpoint: String,
    pub(super) csrf: String,
    pub(super) scope: &'static str,
    pub(super) on_request_check: Callback<()>,
    pub(super) on_discard: Callback<()>,
    pub(super) on_open_retry: Callback<()>,
    pub(super) on_close_retry: Callback<()>,
    pub(super) on_retry_uncertain: Callback<bool>,
    pub(super) on_retry_new_thread: Callback<bool>,
    pub(super) on_retry: Callback<MouseEvent>,
}

#[function_component(AiReviewRecovery)]
pub(super) fn ai_review_recovery(props: &AiReviewRecoveryProps) -> Html {
    let check = {
        let callback = props.on_request_check.clone();
        Callback::from(move |_: MouseEvent| callback.emit(()))
    };
    html! {
        <>
            if props.request_present && (props.request_unresolved || props.request_error.is_some()) {
                <div class="space-y-2 rounded-xl border border-warning/40 p-3" role="status" data-testid="question-bank-ai-request-unknown">
                    <p>{props.request_error.clone().unwrap_or_else(|| "正在查询此前请求；结果确认前不会再次发送。".into())}</p>
                    <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-request-check"
                        disabled={props.submitting} onclick={check.clone()}>{"查询请求状态"}</button>
                    if props.request_not_found {
                        if !props.running {
                            <p class="text-xs text-warning">{"清除仅放弃追踪该请求 ID，不会删除服务端 AI 解析，也不会自动重发；原有输入草稿将保留。"}</p>
                            <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-request-discard"
                                disabled={props.submitting}
                                onclick={Callback::from({let callback=props.on_discard.clone(); move |_: MouseEvent| callback.emit(())})}>{"清除失效请求标识"}</button>
                        } else {
                            <p class="text-xs text-warning">{"当前题目仍在生成，不能清除请求标识；请先停止或查询任务状态。"}</p>
                        }
                    }
                </div>
            }
            if props.disconnected && props.request_present && !props.request_unresolved {
                <button type="button" class="btn btn-ghost btn-sm" data-testid="question-bank-ai-request-refresh"
                    disabled={props.submitting} onclick={check}>{"查询本次请求状态"}</button>
            }
            if props.can_retry {
                if !props.retry_open {
                    <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-restart"
                        disabled={props.submitting}
                        onclick={Callback::from({let callback=props.on_open_retry.clone(); move |_: MouseEvent| callback.emit(())})}>
                        {if props.info.as_ref().is_some_and(|info| info.intent == "follow_up") {"重试本次追问"} else {"重试首次解析"}}
                    </button>
                } else if let Some(info) = props.info.as_ref() {
                    <div class="space-y-2 rounded-xl border border-warning/40 p-3" data-testid="question-bank-ai-retry-confirmation">
                        <p>{if info.intent == "follow_up" {"将使用原追问和原有上下文重试，不重复插入追问。"} else {"将使用原要求重试首次解析。"}}</p>
                        if info.remote_uncertain {
                            <label class="flex items-start gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm" checked={props.confirm_uncertain} data-testid="question-bank-ai-confirm-uncertain"
                                    onchange={Callback::from({let callback=props.on_retry_uncertain.clone(); move |event: Event| {let input: HtmlInputElement=event.target_unchecked_into(); callback.emit(input.checked());}})}/>
                                <span>{"远端执行结果不确定，可能重复消耗额度；我确认仍要重试。"}</span>
                            </label>
                        }
                        if info.error.as_deref().is_some_and(|error| error.starts_with("无法恢复原 Codex Thread")) {
                            <label class="flex items-start gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm" checked={props.confirm_new_thread} data-testid="question-bank-ai-confirm-new-thread"
                                    onchange={Callback::from({let callback=props.on_retry_new_thread.clone(); move |event: Event| {let input: HtmlInputElement=event.target_unchecked_into(); callback.emit(input.checked());}})}/>
                                <span>{"原对话上下文无法恢复；我确认创建新对话后重试。"}</span>
                            </label>
                        }
                        if props.retry_expired {
                            <p class="text-sm text-warning" role="alert">{"原临时图片已失效，请重新上传与原请求相同数量的图片，然后确认重试。"}</p>
                            <AiImagePicker endpoint={ai_scope_url(&props.endpoint, "/attachments", props.scope, None)} csrf={props.csrf.clone()}
                                attachments={props.retry_images.clone()} uploading={props.retry_uploading.clone()} disabled={props.submitting}
                                testid="question-bank-ai-retry-images">
                                <span class="text-sm">{"重新上传原请求的图片"}</span>
                            </AiImagePicker>
                        }
                        <button type="button" class="btn btn-primary btn-sm" data-testid="question-bank-ai-retry-confirm"
                            disabled={props.submitting || *props.retry_uploading || (info.remote_uncertain && !props.confirm_uncertain)
                                || (info.error.as_deref().is_some_and(|error| error.starts_with("无法恢复原 Codex Thread")) && !props.confirm_new_thread)
                                || (props.retry_expired && props.retry_images.is_empty())} onclick={props.on_retry.clone()}>{"确认重试"}</button>
                        <button type="button" class="btn btn-ghost btn-sm" disabled={props.submitting}
                            onclick={Callback::from({let callback=props.on_close_retry.clone(); move |_: MouseEvent| callback.emit(())})}>{"取消"}</button>
                    </div>
                }
            }
        </>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct AiReviewDeleteDialogProps {
    pub(super) visible: bool,
    pub(super) confirmation_required: bool,
    pub(super) confirm_uncertain: bool,
    pub(super) error: Option<String>,
    pub(super) submitting: bool,
    pub(super) on_cancel: Callback<MouseEvent>,
    pub(super) on_confirm: Callback<MouseEvent>,
    pub(super) on_confirm_uncertain: Callback<bool>,
}

#[function_component(AiReviewDeleteDialog)]
pub(super) fn ai_review_delete_dialog(props: &AiReviewDeleteDialogProps) -> Html {
    if !props.visible {
        return Html::default();
    }
    html! {
        <div class="absolute inset-0 z-20 flex items-center justify-center bg-black/40 p-4" role="dialog" aria-modal="true"
            aria-label="确认删除 AI 解析" data-testid="question-bank-ai-delete-dialog">
            <div class="w-full max-w-sm space-y-4 rounded-box bg-base-100 p-5 shadow-xl">
                <p>{"确定删除这道题的 AI 解析和全部追问吗？"}</p>
                if props.confirmation_required {
                    <p class="text-sm text-warning" role="alert">{"远端执行结果不确定，可能仍在运行；删除将清空本地 AI 解析和追问记录，请确认后继续。"}</p>
                    <label class="flex items-start gap-2 text-sm">
                        <input type="checkbox" class="checkbox checkbox-sm"
                            checked={props.confirm_uncertain}
                            data-testid="question-bank-ai-delete-confirm-uncertain"
                            onchange={Callback::from({let callback=props.on_confirm_uncertain.clone(); move |event: Event| {
                                let input: HtmlInputElement=event.target_unchecked_into();
                                callback.emit(input.checked());
                            }})}/>
                        <span>{"我确认清除可能仍在远端执行的 AI 请求记录。"}</span>
                    </label>
                }
                if let Some(error) = props.error.as_ref() {
                    <p class="text-sm text-error" role="alert" data-testid="question-bank-ai-delete-error">{error.clone()}</p>
                }
                <div class="flex justify-end gap-2">
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="question-bank-ai-delete-cancel" disabled={props.submitting}
                        onclick={props.on_cancel.clone()}>{"取消"}</button>
                    <button type="button" class="btn btn-error btn-sm" data-testid="question-bank-ai-delete-confirm"
                        disabled={props.submitting || (props.confirmation_required && !props.confirm_uncertain)}
                        onclick={props.on_confirm.clone()}>{"确认删除"}</button>
                </div>
            </div>
        </div>
    }
}
