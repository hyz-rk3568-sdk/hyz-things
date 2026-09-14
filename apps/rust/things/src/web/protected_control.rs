use super::*;

pub(crate) fn expire_protected_auth(
    state: &UseReducerHandle<AppState>,
    epoch: u64,
    error: &str,
) {
    if error.contains("HTTP 401") {
        state.dispatch(Action::AuthenticationExpired(
            epoch,
            "登录已失效，请重新登录后继续当前页面".to_owned(),
        ));
    }
}

pub(crate) fn dispatch_protected_control<T>(
    state: UseReducerHandle<AppState>,
    area: ControlArea,
    endpoint: &'static str,
    csrf_token: String,
    body: T,
    success: String,
) where
    T: serde::Serialize + 'static,
{
    state.dispatch(Action::ControlStarted(area));
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json(endpoint, &csrf_token, &body, "控制")
            .await
            .map(|_| success);
        if let Err(error) = &result {
            expire_protected_auth(&state, epoch, error);
        }
        let ok = result.is_ok();
        state.dispatch(Action::ControlFinished(area, result));
        if ok {
            match area {
                ControlArea::Display => dispatch_panel_refresh(state.clone()),
                ControlArea::LanTun | ControlArea::LocalSystemProxy => {
                    dispatch_status_refresh(state.clone());
                    dispatch_devices_refresh(state.clone());
                }
                ControlArea::Nodes => {}
            }
        }
    });
}

pub(crate) fn dispatch_protected_delay_refresh(
    state: UseReducerHandle<AppState>,
    csrf_token: String,
) {
    state.dispatch(Action::ControlStarted(ControlArea::Nodes));
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json_response::<_, DelayRefreshControlResponse>(
            PROXY_DELAYS_ENDPOINT,
            &csrf_token,
            &ProxyDelayRefreshRequest {},
            "代理测速",
        )
        .await
        .and_then(|response| match response {
            DelayRefreshControlResponse::ProxyDelays { groups } => Ok(groups),
        });
        if let Err(error) = &result {
            expire_protected_auth(&state, epoch, error);
        }
        state.dispatch(Action::ProxyDelaysFinished(result));
    });
}
