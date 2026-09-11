use super::*;

pub(crate) fn render_activity() -> Html {
    html! {
        <SectionCard title_id="activity-title">
            <PageHeader title_id="activity-title" eyebrow="ACTIVITY" title="活动">
                <span class={SECTION_META}>{"实时连接与近期历史"}</span>
            </PageHeader>
            <div class="flex flex-wrap items-center gap-2">
                <button type="button" class="btn btn-sm btn-primary" aria-pressed="true">{"全部设备"}</button>
            </div>
            <p class={HELP_TEXT}>{"仅展示 LAN 设备的连接元数据；不会记录请求内容。"}</p>
        </SectionCard>
    }
}
