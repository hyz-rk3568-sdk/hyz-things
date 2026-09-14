use super::*;

pub(crate) fn render_study() -> Html {
    html! {
        <>
            <div class={VIEW_HEADING}>
                <div>
                    <p class={EYEBROW}>{"STUDY"}</p>
                    <h2 id="study-title" class={SECTION_TITLE}>{"学习"}</h2>
                </div>
                <span class={SECTION_META}>{"倒计时与备考节奏"}</span>
            </div>
            <CustomCountdownPanel />
            <ExamCountdownPanel />
        </>
    }
}
