include!("app_impl.rs");

#[cfg(test)]
mod selector_contract_tests {
    use super::*;

    #[test]
    fn camera_tab_selector_stays_stable() {
        assert_eq!(AppPage::Camera.tab_id(), "app-camera-tab");
    }
}
