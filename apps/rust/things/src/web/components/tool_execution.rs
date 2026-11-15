//! Shared command/tool rendering for saved AI analysis and ephemeral question
//! generation. Keep typography, indentation and output layout identical.
use yew::prelude::*;

pub(crate) fn tool_execution_item(
    label: &str,
    command: &str,
    output: &str,
    test_prefix: &str,
) -> Html {
    html! {
        <section data-testid={format!("{test_prefix}-tool")} class="min-w-0 max-w-full">
            <p>{label.to_owned()}</p>
            if !command.is_empty() {
                <pre class="mt-1 min-w-0 max-w-full whitespace-pre-wrap break-all" data-testid={format!("{test_prefix}-command")}>{command.to_owned()}</pre>
            }
            if !output.is_empty() {
                <pre class="mt-1 min-w-0 max-w-full whitespace-pre-wrap break-all" data-testid={format!("{test_prefix}-output")}>{output.to_owned()}</pre>
            }
        </section>
    }
}
