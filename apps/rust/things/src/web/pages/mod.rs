use super::*;

mod activity;
mod apps;
mod camera;
mod network;
mod overview;
mod proxy;
mod system;
mod tailscale;

pub(crate) use activity::*;
pub(crate) use apps::*;
pub(crate) use camera::*;
pub(crate) use network::*;
pub(crate) use overview::*;
pub(crate) use proxy::*;
pub(crate) use system::*;
pub(crate) use tailscale::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn study_is_a_first_class_portal_page() {
        assert!(AppPage::ALL.contains(&AppPage::Study));
        assert_eq!(AppPage::Study.label(), "学习");
        assert_eq!(AppPage::Study.tab_id(), "app-study-tab");
        assert_eq!(AppPage::Study.panel_id(), "app-study-panel");
    }
}
