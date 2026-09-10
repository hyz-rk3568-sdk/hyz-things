pub const PAGE: &str = "relative mx-auto min-h-screen w-full max-w-7xl px-4 pb-[max(1.5rem,env(safe-area-inset-bottom))] pt-[max(1rem,env(safe-area-inset-top))] sm:px-6 lg:px-8";
pub const APP_HEADER: &str =
    "flex flex-col gap-4 py-3 sm:flex-row sm:items-center sm:justify-between";
pub const BRAND: &str = "flex min-w-0 items-center gap-3";
pub const BRAND_MARK: &str = "grid size-11 shrink-0 place-items-center rounded-box bg-primary font-mono text-xs font-black tracking-wider text-primary-content shadow-lg shadow-primary/15";
pub const EYEBROW: &str = "mb-1 text-[0.65rem] font-bold uppercase tracking-[0.2em] text-primary";
pub const PAGE_TITLE: &str = "text-2xl font-black tracking-tight text-base-content sm:text-3xl";
pub const SUBTITLE: &str = "mt-1 text-xs text-base-content/65 sm:text-sm";
pub const OVERALL: &str = "flex min-w-48 items-center gap-3 rounded-box border border-base-content/10 bg-base-200/80 px-4 py-3 shadow-sm";
pub const OVERALL_COPY: &str = "grid gap-0.5";
pub const OVERALL_TITLE: &str = "text-sm font-bold";
pub const OVERALL_META: &str = "text-xs text-base-content/65";
pub const STATUS_DOT: &str = "size-2 shrink-0 rounded-full bg-current";
pub const STATUS_DOT_SMALL: &str = "size-1.5 shrink-0 rounded-full bg-current";

pub const WORKSPACE_TABS: &str = "tabs tabs-box sticky top-2 z-20 mt-3 grid grid-cols-2 overflow-x-auto border border-base-content/10 bg-base-200/90 p-1 shadow-lg shadow-black/10 backdrop-blur";
pub const WORKSPACE_TAB: &str = "tab h-10 min-w-24 rounded-field text-xs font-bold sm:text-sm";
pub const WORKSPACE_TAB_ACTIVE: &str = "tab-active bg-primary text-primary-content";
pub const WORKSPACE_PANEL: &str = "min-w-0 outline-none";

pub const PORTAL_TABS: &str = "tabs tabs-box sticky top-2 z-20 mt-3 grid grid-cols-3 overflow-x-auto border border-base-content/10 bg-base-200/90 p-1 shadow-lg shadow-black/10 backdrop-blur";
pub const PORTAL_TAB: &str = "tab h-10 min-w-24 rounded-field text-xs font-bold sm:text-sm";
pub const PORTAL_TAB_ACTIVE: &str = "tab-active bg-primary text-primary-content";

pub const PORTAL_SWIPE_SURFACE: &str = "min-w-0 touch-pan-y";

pub const APP_GRID: &str = "grid grid-cols-1 gap-4 lg:grid-cols-3";
pub const APP_CARD: &str =
    "card min-w-0 gap-4 border border-base-content/10 bg-base-100/60 p-5 shadow-sm";

pub const EXAM_SECTION: &str =
    "card relative isolate overflow-hidden border border-base-content/10 bg-base-300/95 p-4 shadow-xl shadow-black/20 sm:p-6";
pub const EXAM_DECORATION: &str =
    "pointer-events-none absolute -right-16 -top-16 -z-10 size-48 rounded-full bg-base-content/5 blur-3xl";
pub const EXAM_HEAD: &str = "flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between";
pub const EXAM_HEAD_META: &str =
    "flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-base-content/65";
pub const EXAM_BADGE: &str =
    "badge badge-outline gap-1.5 border-base-content/20 bg-base-content/5 text-[0.65rem] font-bold text-base-content/70";
pub const EXAM_GRID: &str = "mt-5 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4";
pub const EXAM_CARD: &str =
    "card min-w-0 touch-manipulation gap-3 border border-base-content/10 bg-base-300/90 p-4 shadow-lg shadow-black/20 transition-shadow";
pub const EXAM_PIP_NOTICE: &str =
    "mt-3 rounded-box border border-base-content/10 bg-base-200/70 px-3 py-2 text-xs text-base-content/70";
pub const EXAM_COUNTER_VALUE: &str =
    "text-3xl font-black leading-none tracking-tighter sm:text-4xl";
pub const EXAM_CARD_URGENT: &str = "border-error/60 bg-base-300/95 shadow-black/25";
pub const EXAM_CARD_SOON: &str = "border-warning/60 bg-base-300/95 shadow-black/25";
pub const EXAM_CARD_FINISHED: &str = "border-base-content/10 bg-base-300/60 opacity-75 shadow-sm";
pub const EXAM_CARD_HEAD: &str = "flex min-w-0 items-start justify-between gap-2";
pub const EXAM_CARD_INDEX: &str =
    "grid size-7 shrink-0 place-items-center rounded-full bg-base-content/10 font-mono text-xs font-black text-base-content/75";
pub const EXAM_CARD_COPY: &str = "grid min-w-0 gap-0.5";
pub const EXAM_CARD_EYEBROW: &str =
    "text-[0.6rem] font-bold uppercase tracking-wider text-base-content/60";
pub const EXAM_CARD_TITLE: &str = "truncate text-base font-black text-base-content/85";
pub const EXAM_STATUS: &str = "badge badge-outline shrink-0 text-[0.6rem] font-bold";
pub const EXAM_COUNTER: &str =
    "flex min-w-0 items-baseline gap-1 overflow-hidden font-mono text-base-content/80";
pub const EXAM_COUNTER_UNIT: &str = "text-xs font-bold text-base-content/55";
pub const EXAM_COUNTER_FINISHED: &str = "text-2xl font-black leading-none text-base-content/65";
pub const EXAM_META: &str =
    "flex items-center justify-between gap-2 text-[0.65rem] text-base-content/60";
pub const EXAM_DATE: &str = "shrink-0 font-mono font-bold text-base-content/75";
pub const EXAM_PROGRESS: &str = "progress progress-neutral h-2 w-full bg-base-content/10";
pub const EXAM_PROGRESS_META: &str =
    "flex items-center justify-between gap-2 text-[0.6rem] text-base-content/60";

pub const TOPOLOGY: &str =
    "card mt-5 gap-4 border border-base-content/10 bg-base-200/70 p-4 shadow-sm sm:p-6";
pub const TOPOLOGY_FLOW: &str =
    "grid grid-cols-1 items-stretch gap-2 lg:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)] lg:items-center";
pub const TOPOLOGY_NODE: &str = "grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-3 rounded-box bg-base-100/60 p-4 ring-1 ring-base-content/10";
pub const TOPOLOGY_ICON: &str = "grid size-9 place-items-center rounded-box border border-current/25 bg-current/5 font-mono text-[0.6rem] font-black";
pub const TOPOLOGY_COPY: &str = "grid min-w-0 gap-1";
pub const TOPOLOGY_TITLE: &str = "text-sm font-bold";
pub const TOPOLOGY_DETAIL: &str = "truncate text-xs text-base-content/65";
pub const TOPOLOGY_LINK: &str =
    "grid place-items-center text-lg font-black max-lg:rotate-90 lg:px-1";
pub const TOPOLOGY_UPLINKS: &str = "grid min-w-0 gap-2";
pub const TOPOLOGY_UPLINKS_HEAD: &str = "flex items-center justify-between gap-3 px-1 text-[0.65rem] font-bold uppercase tracking-wider text-base-content/60";
pub const TOPOLOGY_UPLINK_BADGE: &str =
    "badge badge-success badge-outline shrink-0 text-[0.6rem] font-bold";
pub const TOPOLOGY_BRANCH_STACK: &str =
    "grid min-w-0 gap-2 lg:col-span-7 lg:mx-auto lg:w-full lg:max-w-xl";
pub const TOPOLOGY_PROXY_ROW: &str =
    "grid min-w-0 grid-cols-[auto_minmax(0,1fr)] items-center gap-3";
pub const TOPOLOGY_BRANCH: &str = "grid size-8 place-items-center text-xl text-secondary";

pub const KPI_GRID: &str = "mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4";
pub const KPI_HEAD: &str = "flex min-w-0 items-center justify-between gap-2";
pub const KPI_CARD: &str =
    "grid min-w-0 gap-1 rounded-box border border-base-content/10 bg-base-100/50 p-4 shadow-sm";
pub const KPI_LABEL: &str =
    "text-[0.65rem] font-bold uppercase tracking-wider text-base-content/65";
pub const KPI_VALUE: &str = "truncate font-mono text-sm font-black text-base-content sm:text-base";
pub const KPI_META: &str = "text-[0.65rem] text-base-content/65";
pub const VIEW_HEADING: &str =
    "mt-6 flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between";

pub const NOTICE: &str = "alert mb-5 items-start border text-sm shadow-sm sm:grid-cols-[auto_1fr]";
pub const NOTICE_COPY: &str = "text-base-content/70";
pub const DASHBOARD: &str = "grid grid-cols-1 gap-4 lg:grid-cols-3";
pub const LOADING_GRID: &str = "grid grid-cols-1 gap-4 lg:grid-cols-3";
pub const SKELETON: &str =
    "skeleton h-72 w-full rounded-box bg-base-200 motion-reduce:animate-none";
pub const EMPTY_STATE: &str = "card items-center border border-dashed border-base-content/20 bg-base-200/50 px-6 py-16 text-center shadow-sm";
pub const EMPTY_ICON: &str = "mb-3 grid size-11 place-items-center rounded-full border border-error/30 bg-error/10 text-lg font-black text-error";
pub const EMPTY_TITLE: &str = "text-lg font-bold text-base-content";
pub const EMPTY_COPY: &str = "mt-2 text-sm text-base-content/60";

pub const SECTION: &str = "card mt-5 gap-4 border border-base-content/10 bg-base-200/70 p-4 shadow-xl shadow-black/10 sm:p-6";
pub const SECTION_HEAD: &str = "flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between";
pub const SECTION_HEAD_CENTERED: &str =
    "flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between";
pub const SECTION_TITLE: &str = "text-xl font-black tracking-tight";
pub const SECTION_META: &str = "text-xs text-base-content/65";
pub const FEEDBACK: &str =
    "alert min-h-12 border border-base-content/10 bg-base-100/50 text-xs text-base-content/70";
pub const INNER_CARD: &str =
    "card min-w-0 gap-4 border border-base-content/10 bg-base-100/60 p-4 shadow-sm sm:p-5";
pub const CUSTOM_TIMER_GRID: &str =
    "mt-4 grid min-w-0 grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3";
pub const CUSTOM_TIMER_CARD: &str =
    "card min-w-0 gap-3 border border-base-content/10 bg-base-100/60 p-3 shadow-sm sm:p-4";
pub const CUSTOM_TIMER_HEAD: &str = "flex min-w-0 items-center justify-between gap-2";
pub const CUSTOM_TIMER_TITLE: &str = "text-base font-black";
pub const CUSTOM_TIMER_INPUTS: &str = "grid grid-cols-3 gap-2";
pub const CUSTOM_TIMER_ACTIONS: &str = "flex flex-wrap gap-2";
pub const CONTROL_TITLE: &str =
    "flex flex-col gap-1 sm:flex-row sm:items-baseline sm:justify-between";
pub const CONTROL_HEADING: &str = "text-sm font-bold";
pub const CONTROL_META: &str = "text-xs text-base-content/65";
pub const RANGE_LABEL: &str = "flex items-center justify-between text-xs text-base-content/60";
pub const RANGE: &str = "range range-primary range-sm w-full";
pub const BUTTON_ROW: &str = "flex flex-wrap gap-2";
pub const BUTTON: &str = "btn btn-sm";
pub const BUTTON_PRIMARY: &str = "btn btn-primary btn-sm";
pub const BUTTON_ERROR: &str = "btn btn-error btn-sm";
pub const BUTTON_GHOST: &str = "btn btn-ghost btn-sm";
pub const BUTTON_BLOCK_MOBILE: &str = "btn btn-primary btn-sm max-sm:w-full";
pub const HELP_TEXT: &str = "text-xs leading-relaxed text-base-content/65";

pub const CAMERA_CARD: &str =
    "card mb-5 gap-4 border border-base-content/10 bg-base-100/60 p-4 shadow-sm sm:p-5";
pub const CAMERA_LAYOUT: &str = "grid min-w-0 grid-cols-1 gap-4 lg:grid-cols-2 lg:items-center";
pub const CAMERA_COPY: &str = "grid min-w-0 gap-4";
pub const CAMERA_MEDIA: &str = "min-w-0";
pub const CAMERA_METRICS: &str = "grid grid-cols-1 gap-2 text-xs sm:grid-cols-2";
pub const CAMERA_METRIC: &str = "grid grid-cols-[auto_minmax(0,1fr)] gap-3 rounded-box border border-base-content/10 bg-base-200/50 px-3 py-2 text-base-content/65 [&_dd]:truncate [&_dd]:text-right [&_dd]:font-mono [&_dd]:font-semibold [&_dd]:text-base-content";
pub const CAMERA_NOTICE: &str =
    "rounded-box border border-base-content/10 bg-base-200/50 px-3 py-2 text-xs text-base-content/70";
pub const CAMERA_STAGE: &str = "relative min-w-0 overflow-hidden rounded-box border border-base-content/10 bg-black shadow-inner";
pub const CAMERA_VIDEO: &str = "aspect-video block w-full bg-black object-contain";
pub const CAMERA_CONTROLS: &str = "mt-3 flex flex-wrap items-center justify-center gap-2";
pub const CAMERA_CONTROL_BUTTON: &str = "btn btn-circle btn-sm touch-none select-none border-base-content/15 bg-base-200/80 shadow-sm transition-colors hover:bg-base-300 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary";
pub const CAMERA_CONTROL_BUTTON_ACTIVE: &str =
    "bg-primary text-primary-content hover:bg-primary/90";
pub const CAMERA_CONTROL_BUTTON_DANGER: &str = "bg-error text-error-content hover:bg-error/90";
pub const CAMERA_CONTROL_BUTTON_DISABLED: &str = "cursor-not-allowed opacity-50";
pub const CAMERA_ICON: &str = "size-5";
pub const CAMERA_PLACEHOLDER: &str = "absolute inset-0 grid place-content-center place-items-center gap-3 bg-base-300 text-xs font-semibold text-base-content/60";
pub const CAMERA_PLACEHOLDER_ICON: &str = "grid size-11 place-items-center rounded-full border border-primary/30 bg-primary/10 font-mono text-[0.6rem] font-black text-primary";

pub const PROXY_GROUPS: &str = "grid gap-3";
pub const PROXY_TOOLBAR: &str = "flex flex-col gap-2 text-xs text-base-content/65 sm:flex-row sm:items-center sm:justify-between";
pub const PROXY_GROUP: &str = "grid min-w-0 grid-cols-1 gap-3 rounded-box border border-base-content/10 bg-base-100/60 p-4 sm:grid-cols-[minmax(8rem,0.7fr)_minmax(12rem,1.5fr)] sm:items-center";
pub const PROXY_NAME_WRAP: &str = "min-w-0";
pub const PROXY_NAME: &str = "truncate text-sm font-bold";
pub const PROXY_KIND: &str = "text-xs text-base-content/65";
pub const SELECT: &str = "select select-bordered select-sm w-full min-w-0 bg-base-100";

pub const SETTINGS_GRID: &str = "grid grid-cols-1 gap-4 lg:grid-cols-2";
pub const DISCLOSURE: &str =
    "min-w-0 overflow-hidden rounded-box border border-base-content/10 bg-base-100/60 shadow-sm";
pub const DISCLOSURE_TOGGLE: &str = "flex min-h-20 w-full items-center justify-between gap-4 px-5 py-4 text-left hover:bg-base-content/5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-primary";
pub const DISCLOSURE_COPY: &str = "grid min-w-0 gap-1";
pub const DISCLOSURE_TITLE: &str = "text-sm font-bold";
pub const DISCLOSURE_SUMMARY: &str = "truncate text-xs text-base-content/65";
pub const DISCLOSURE_ACTION: &str = "shrink-0 text-xs font-semibold text-primary";
pub const DISCLOSURE_DETAIL: &str = "border-t border-base-content/10 px-5 pb-5";
pub const LOGIN_DISCLOSURE: &str =
    "max-w-3xl overflow-hidden rounded-box border border-base-content/10 bg-base-100/60 shadow-sm";
pub const AUTH_FORM: &str = "grid max-w-3xl grid-cols-1 items-end gap-4 pt-5 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]";
pub const FORM_GRID: &str = "grid grid-cols-1 gap-4 md:grid-cols-3";
pub const FORM_GRID_COMPACT: &str = "grid grid-cols-1 gap-4 sm:grid-cols-2";
pub const FIELD: &str = "fieldset min-w-0 p-0";
pub const FIELD_LABEL: &str = "fieldset-legend pb-1 text-xs font-semibold text-base-content/65";
pub const INPUT: &str = "input input-bordered input-sm w-full bg-base-100";
pub const READONLY_INPUT: &str =
    "input input-bordered input-sm w-full bg-base-200 text-base-content/65";
pub const FORM_ACTIONS: &str = "flex flex-wrap items-center gap-2 md:col-span-full";
pub const SESSION_ACTIONS: &str =
    "flex items-center justify-between gap-3 text-xs text-base-content/60 sm:justify-end";
pub const FORCED_PASSWORD: &str = "grid gap-4";
pub const RISK_ALERT: &str = "alert items-start border text-sm";
pub const RISK_COPY: &str = "text-base-content/70";
pub const RISK_NOTE: &str =
    "alert alert-warning border border-warning/20 py-2 text-xs leading-relaxed sm:col-span-2";
pub const SETTINGS_EMPTY: &str = "rounded-box border border-dashed border-base-content/20 bg-base-100/40 p-6 text-center text-sm text-base-content/65";
pub const DETAIL_TOOLBAR: &str =
    "flex flex-col gap-3 py-4 sm:flex-row sm:items-center sm:justify-between";
pub const SCAN_LIST: &str = "mb-4 grid max-h-52 gap-2 overflow-y-auto";
pub const SCAN_ENTRY: &str = "btn h-auto min-h-12 justify-between gap-3 px-3 py-2 text-left max-sm:flex-col max-sm:items-start";
pub const SCAN_NAME: &str = "min-w-0 truncate";
pub const SCAN_META: &str = "shrink-0 text-[0.65rem] font-medium text-base-content/65";
pub const SUMMARY: &str = "flex items-center justify-between rounded-box border border-base-content/10 bg-base-200/50 px-3 py-2 text-xs text-base-content/60";
pub const PENDING_AP: &str = "grid gap-4";
pub const CONFIG_SUMMARY: &str =
    "grid grid-cols-[auto_minmax(0,1fr)] items-baseline gap-x-4 gap-y-1 py-3 text-xs";
pub const CONFIG_LABEL: &str = "text-base-content/65";
pub const CONFIG_VALUE: &str = "truncate text-right font-mono font-semibold text-base-content";
pub const CONFIG_META: &str = "col-span-2 text-right text-base-content/65";

pub const CONFIRMATION_PANEL: &str =
    "grid gap-4 rounded-box border border-error/30 bg-error/5 p-4 shadow-sm sm:p-5";
pub const CONFIRMATION_TITLE: &str = "text-lg font-black";
pub const CONFIRMATION_COPY: &str = "text-sm leading-relaxed text-base-content/70";
pub const CONFIRMATION_ACTIONS: &str = "flex flex-col-reverse gap-2 sm:flex-row sm:justify-end";

pub const STATUS_CARD: &str = "card min-w-0 border border-base-content/10 bg-base-100/45 shadow-sm";
pub const STATUS_CARD_BODY: &str = "card-body gap-4 p-5";
pub const STATUS_CARD_HEAD: &str = "grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3";
pub const STATUS_ICON: &str = "grid size-9 place-items-center rounded-box border border-current/20 bg-current/5 font-mono text-[0.65rem] font-black";
pub const STATUS_TITLE: &str = "text-sm font-bold";
pub const STATUS_BADGE: &str =
    "badge badge-outline gap-1.5 whitespace-nowrap text-[0.65rem] font-semibold";
pub const METRIC_LIST: &str = "m-0 divide-y divide-base-content/10";
pub const METRIC: &str = "grid grid-cols-[minmax(5.5rem,0.85fr)_minmax(0,1.5fr)] items-baseline gap-4 py-2.5 first:pt-0 last:pb-0";
pub const METRIC_LABEL: &str = "text-xs text-base-content/65";
pub const METRIC_VALUE: &str =
    "min-w-0 truncate text-right font-mono text-xs font-medium text-base-content";
pub const WARNINGS: &str = "alert alert-warning mt-5 items-start border border-warning/20 text-sm";
pub const WARNINGS_LIST: &str = "mt-2 list-disc space-y-1 pl-5 text-base-content/70";
pub const FOOTER: &str = "px-2 py-7 text-center text-xs text-base-content/65";

pub const ACCENT_WAN: &str = "border-t-2 border-t-info";
pub const ACCENT_PROXY: &str = "border-t-2 border-t-warning";
pub const ACCENT_SYSTEM: &str = "border-t-2 border-t-primary";
pub const ICON_WAN: &str = "text-info";
pub const ICON_PROXY: &str = "text-warning";
pub const ICON_SYSTEM: &str = "text-primary";
