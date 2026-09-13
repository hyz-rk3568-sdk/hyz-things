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

#[cfg(test)]
impl std::fmt::Debug for AppPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            AppPage::Overview => "Overview",
            AppPage::Study => "Study",
            AppPage::Network => "Network",
            AppPage::Proxy => "Proxy",
            AppPage::Activity => "Activity",
            AppPage::Tailscale => "Tailscale",
            AppPage::Camera => "Camera",
            AppPage::Apps => "Apps",
            AppPage::System => "System",
        })
    }
}
