//! Shared Question Bank / 申论 AI composer and attachment picker.
//! Preserve the Question Bank theme, focus ring, image previews and send/stop affordance.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct PendingImage {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) preview: Option<Rc<String>>,
}

async fn image_preview_data(file: &web_sys::File) -> Option<String> {
    use base64::Engine as _;
    let value = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
        .await
        .ok()?;
    let bytes = js_sys::Uint8Array::new(&value).to_vec();
    Some(format!(
        "data:{};base64,{}",
        file.type_(),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[derive(Properties, PartialEq)]
pub(super) struct AiImagePickerProps {
    pub(super) endpoint: String,
    pub(super) csrf: String,
    pub(super) attachments: UseStateHandle<Vec<PendingImage>>,
    pub(super) uploading: UseStateHandle<bool>,
    pub(super) disabled: bool,
    pub(super) testid: &'static str,
    #[prop_or(false)]
    pub(super) compact: bool,
    pub(super) children: Children,
}

#[function_component(AiImagePicker)]
pub(super) fn ai_image_picker(props: &AiImagePickerProps) -> Html {
    let failure = use_state(|| None::<String>);
    let upload = {
        let endpoint = props.endpoint.clone();
        let csrf = props.csrf.clone();
        let attachments = props.attachments.clone();
        let uploading = props.uploading.clone();
        let failure = failure.clone();
        let disabled = props.disabled;
        Callback::from(move |files: Vec<web_sys::File>| {
            if disabled || *uploading {
                return;
            }
            let remaining = 3usize.saturating_sub(attachments.len());
            let mut selected = Vec::new();
            for file in files {
                if selected.len() >= remaining {
                    break;
                }
                if !matches!(file.type_().as_str(), "image/png" | "image/jpeg") {
                    failure.set(Some("仅支持 JPG 或 PNG 图片".into()));
                    continue;
                }
                if file.size() > (8 * 1024 * 1024) as f64 {
                    failure.set(Some("每张图片不能超过 8 MiB".into()));
                    continue;
                }
                selected.push(file);
            }
            if selected.is_empty() {
                return;
            }
            failure.set(None);
            uploading.set(true);
            let attachments = attachments.clone();
            let uploading = uploading.clone();
            let failure = failure.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let mut next = (*attachments).clone();
                for file in selected {
                    match upload_question_ai_image(&endpoint, &csrf, file.clone()).await {
                        Ok(id) => next.push(PendingImage {
                            id,
                            name: file.name(),
                            preview: image_preview_data(&file).await.map(Rc::new),
                        }),
                        Err(error) => {
                            failure.set(Some(error));
                            break;
                        }
                    }
                }
                attachments.set(next);
                uploading.set(false);
            });
        })
    };
    let onchange = {
        let upload = upload.clone();
        Callback::from(move |event: Event| {
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            let files = input
                .files()
                .map(|files| (0..files.length()).filter_map(|i| files.get(i)).collect())
                .unwrap_or_default();
            input.set_value("");
            upload.emit(files);
        })
    };
    let onpaste = Callback::from(move |event: Event| {
        let event: web_sys::ClipboardEvent = wasm_bindgen::JsCast::unchecked_into(event);
        let Some(data) = event.clipboard_data() else {
            return;
        };
        let Some(files) = data.files() else {
            return;
        };
        let images: Vec<_> = (0..files.length())
            .filter_map(|i| files.get(i))
            .filter(|file| file.type_().starts_with("image/"))
            .collect();
        if !images.is_empty() {
            event.prevent_default();
            upload.emit(images);
        }
    });
    let picker = html! {
        <label class="btn btn-ghost btn-sm btn-circle shrink-0 text-base-content/60 hover:text-base-content"
            title="添加图片（JPG/PNG，最多 3 张，每张 ≤ 8 MiB）" aria-label="添加图片">
            <input type="file" class="hidden" accept="image/png,image/jpeg" multiple=true
                disabled={props.disabled || *props.uploading} onchange={onchange}/>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
        </label>
    };
    let thumbnails = html! { for props.attachments.iter().map(|image| {
        let attachments = props.attachments.clone();
        let id = image.id.clone();
        html! {
            <span key={id.clone()} class="relative h-9 w-9 shrink-0 rounded-md border border-base-content/20" title={image.name.clone()}>
                if let Some(preview) = &image.preview {
                    <img class="h-full w-full rounded-md object-cover" src={(**preview).clone()} alt={image.name.clone()}/>
                } else { <span class="text-xs">{"图"}</span> }
                <button type="button" class="absolute -right-1 -top-1 rounded-full bg-base-300 px-1 text-xs leading-4"
                    aria-label={format!("移除图片 {}",image.name)} disabled={props.disabled || *props.uploading}
                    onclick={Callback::from(move |_| attachments.set(attachments.iter().filter(|value| value.id != id).cloned().collect()))}>{"×"}</button>
            </span>
        }
    }) };
    html! {
        <div class={if props.compact {"px-1 py-1"} else {"space-y-2 pb-2 px-2"}} data-testid={props.testid} {onpaste}>
            if props.compact {
                <div class="flex flex-wrap items-center gap-1 pr-11">{picker}{thumbnails}
                    if *props.uploading { <span class="loading loading-spinner loading-xs" aria-label="正在上传图片"></span> }
                    {for props.children.iter()}
                </div>
            } else {
                {for props.children.iter()}
                <div class="flex flex-wrap items-center gap-2">{picker}{thumbnails}
                    if *props.uploading { <span class="loading loading-spinner loading-xs" aria-label="正在上传图片"></span> }
                </div>
            }
            if let Some(error) = failure.as_ref() { <p class="text-xs text-error" role="alert">{error.clone()}</p> }
        </div>
    }
}
pub(super) struct AiComposer<'a> {
    pub(super) id: &'a str,
    pub(super) input_ref: NodeRef,
    pub(super) compose_testid: &'a str,
    pub(super) input_testid: &'a str,
    pub(super) button_testid: &'a str,
    pub(super) value: &'a str,
    pub(super) max_length: usize,
    pub(super) read_only: bool,
    pub(super) placeholder: &'a str,
    pub(super) button_label: &'a str,
    pub(super) is_stop: bool,
    pub(super) disabled: bool,
    pub(super) endpoint: String,
    pub(super) csrf: String,
    pub(super) attachments: UseStateHandle<Vec<PendingImage>>,
    pub(super) uploading: UseStateHandle<bool>,
    pub(super) picker_disabled: bool,
    pub(super) picker_testid: &'static str,
    pub(super) oninput: Callback<InputEvent>,
    pub(super) onkeydown: Callback<KeyboardEvent>,
    pub(super) onclick: Callback<MouseEvent>,
}

/// One shape for both asking Codex to start a review and sending a follow-up.
pub(super) fn ai_composer(props: AiComposer<'_>) -> Html {
    let oninput = {
        let callback = props.oninput.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
            let _ = input.style().set_property("height", "auto");
            let _ = input.style().set_property(
                "height",
                &format!("{}px", input.scroll_height().clamp(24, 160)),
            );
            callback.emit(event);
        })
    };
    html! {
        <div class="relative rounded-2xl border border-base-content/15 bg-base-100 transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/15"
            data-testid={props.compose_testid.to_owned()}>
            <AiImagePicker endpoint={props.endpoint} csrf={props.csrf} attachments={props.attachments}
                uploading={props.uploading} disabled={props.picker_disabled} testid={props.picker_testid} compact={true}>
            <textarea ref={props.input_ref} id={props.id.to_owned()} rows="1"
                class="block h-6 min-w-16 flex-1 resize-none overflow-y-auto border-0 bg-transparent px-1 py-0 text-sm leading-6 focus:outline-none focus:ring-0"
                data-testid={props.input_testid.to_owned()} maxlength={props.max_length.to_string()} readonly={props.read_only}
                value={props.value.to_owned()}
                placeholder={props.placeholder.to_owned()}
                onkeydown={props.onkeydown}
                {oninput}/>
            </AiImagePicker>
            <div class="absolute right-2 top-1/2 flex -translate-y-1/2 items-center justify-end">
                <button type="button" class="btn btn-primary btn-sm btn-circle" aria-label={props.button_label.to_owned()} title={props.button_label.to_owned()}
                    data-testid={props.button_testid.to_owned()} disabled={props.disabled} onclick={props.onclick}>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        if props.is_stop { <rect x="7" y="7" width="10" height="10" rx="1" fill="currentColor" stroke="none"/> }
                        else { <path d="M12 19V5M5 12l7-7 7 7"/> }
                    </svg>
                </button>
            </div>
        </div>
    }
}
