//! Shared user-prompt settings for AI-generated papers.
use super::*;

#[derive(Clone)]
pub(crate) struct GenerationPromptState {
    pub(crate) text: UseStateHandle<String>,
    pub(crate) customized: UseStateHandle<bool>,
    pub(crate) busy: UseStateHandle<bool>,
    pub(crate) error: UseStateHandle<Option<String>>,
    pub(crate) loaded: UseStateHandle<bool>,
}

#[hook]
pub(crate) fn use_generation_prompt() -> GenerationPromptState {
    GenerationPromptState {
        text: use_state(String::new),
        customized: use_state(|| false),
        busy: use_state(|| false),
        error: use_state(|| None::<String>),
        loaded: use_state(|| false),
    }
}

impl GenerationPromptState {
    pub(crate) async fn load(&self, root: &'static str) {
        match fetch_json_typed::<UserPromptSettings>(&format!("{root}/user-prompt"), "读取出题要求")
            .await
        {
            Ok(saved) => {
                self.text.set(saved.user_prompt);
                self.customized.set(saved.customized);
                self.error.set(None);
                self.loaded.set(true);
            }
            Err(error) => self.error.set(Some(format!("读取出题要求失败：{error}"))),
        }
    }

    pub(crate) fn save(&self, root: &'static str, csrf: String) {
        if *self.busy {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let text = (*self.text).clone();
        let state = self.clone();
        spawn_local(async move {
            match post_json_response_typed::<_, UserPromptSettings>(
                &format!("{root}/user-prompt"),
                &csrf,
                &UserPromptUpdate {
                    user_prompt: Some(text),
                },
                "保存出题要求",
            )
            .await
            {
                Ok(saved) => {
                    state.text.set(saved.user_prompt);
                    state.customized.set(saved.customized);
                }
                Err(error) => state.error.set(Some(format!("保存失败：{error}"))),
            }
            state.busy.set(false);
        });
    }

    pub(crate) fn reset(&self, root: &'static str, csrf: String) {
        if *self.busy {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        let state = self.clone();
        spawn_local(async move {
            match post_json_response_typed::<_, UserPromptSettings>(
                &format!("{root}/user-prompt"),
                &csrf,
                &UserPromptUpdate { user_prompt: None },
                "恢复默认出题要求",
            )
            .await
            {
                Ok(saved) => {
                    state.text.set(saved.user_prompt);
                    state.customized.set(saved.customized);
                }
                Err(error) => state.error.set(Some(format!("恢复默认失败：{error}"))),
            }
            state.busy.set(false);
        });
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct GenerationPromptSettingsProps {
    pub(crate) prefix: AttrValue,
    #[prop_or_default]
    pub(crate) textarea_id: Option<AttrValue>,
    #[prop_or_default]
    pub(crate) aria_label: Option<AttrValue>,
    pub(crate) text: String,
    pub(crate) status_label: AttrValue,
    pub(crate) busy: bool,
    pub(crate) editor_disabled: bool,
    pub(crate) save_disabled: bool,
    pub(crate) reset_disabled: bool,
    #[prop_or_default]
    pub(crate) error: Option<String>,
    pub(crate) save_label: AttrValue,
    pub(crate) busy_label: AttrValue,
    pub(crate) reset_label: AttrValue,
    #[prop_or_default]
    pub(crate) footer: Option<Html>,
    pub(crate) on_input: Callback<InputEvent>,
    pub(crate) on_save: Callback<MouseEvent>,
    pub(crate) on_reset: Callback<MouseEvent>,
    pub(crate) children: Children,
}

#[function_component(GenerationPromptSettings)]
pub(crate) fn generation_prompt_settings(props: &GenerationPromptSettingsProps) -> Html {
    let prefix = props.prefix.clone();
    html! {
        <details class="rounded-box border border-base-content/15 bg-base-100/50 p-3" data-testid={format!("{prefix}-user-prompt")}>
            <summary class="cursor-pointer select-none font-semibold" data-testid={format!("{prefix}-user-prompt-toggle")}>
                {"出题要求"}
                <span class="ml-2 text-xs font-normal text-base-content/60">{props.status_label.clone()}</span>
            </summary>
            <div class="mt-3 space-y-3">
                {for props.children.iter()}
                <textarea id={props.textarea_id.clone()}
                    class="textarea textarea-bordered w-full min-h-48 font-mono text-base leading-6"
                    data-testid={format!("{prefix}-user-prompt-editor")} maxlength="8000"
                    aria-label={props.aria_label.clone()} value={props.text.clone()}
                    oninput={props.on_input.clone()} disabled={props.editor_disabled}>
                </textarea>
                if let Some(message) = props.error.clone() {
                    <p role="alert" class="text-error text-sm">{message}</p>
                }
                <div class="flex flex-wrap justify-end gap-2">
                    <button type="button" class="btn btn-outline btn-sm"
                        data-testid={format!("{prefix}-user-prompt-reset")}
                        disabled={props.reset_disabled} onclick={props.on_reset.clone()}>
                        {props.reset_label.clone()}
                    </button>
                    <button type="button" class="btn btn-primary btn-sm"
                        data-testid={format!("{prefix}-user-prompt-save")}
                        disabled={props.save_disabled} onclick={props.on_save.clone()}>
                        {if props.busy { props.busy_label.clone() } else { props.save_label.clone() }}
                    </button>
                </div>
                if let Some(footer) = props.footer.clone() { {footer} }
            </div>
        </details>
    }
}
