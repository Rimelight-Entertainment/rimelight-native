//! The Slint types compiled from `crates/space_ui/ui/main.slint` (decision 0013). `space_ui` re-exports them, so
//! the rest of the code keeps using `space_ui`'s paths. Editing Rust code in `space_ui` does not rebuild this.

// Slint's generated code contains unsafe vtable statics. Only this module may allow it.
#[allow(unsafe_code)]
mod generated {
    slint::include_modules!();
}

pub use generated::*;

pub mod clipboard;

// Company area window glue (ST5.2, rimelight-rs `rimelight_ui_kit` pattern): `wire_company!` implements the company pages'
// window traits (`space_area_company`) for the generated `AppShell`, and emits the `company_glue` helpers. It is expanded
// here, in the crate that defines `AppShell` (a trait impl for a generated type must live in the crate that generates it).
space_area_company::wire_company!(
    AppShell,
    PageLoadState,
    TableColumn,
    TableRow,
    SelectNav,
    SelectOption,
    EmployeesTable,
    EmployeeRow,
    TeamsTable,
    OrgRow,
    TeamRow,
    TeamView,
    DeptsTable,
    DeptView,
    LocationsState,
    ProfileView,
    ProfileDraft,
    PersonLink,
);

// Management area window glue (ST5.2): the project settings and members pages (`space_area_management`).
space_area_management::wire_management!(
    AppShell,
    PageLoadState,
    ProjectSettingsState,
    ProjectMembersState,
    MemberRow,
    SelectOption
);

// Admin area window glue (SP.3e): Users, Organisation, Roles and Types & tags (`space_area_admin`).
space_area_admin::wire_admin!(
    AppShell,
    PageLoadState,
    SelectOption,
    TableColumn,
    TableRow,
    CalendarDay,
    AdminRow,
    AdminOrgState,
    AdminUserRow,
    AdminUsersState,
    AdminRolesState,
    RolePerson,
    AdminTypesTagsState,
    TypeRow,
    AreaRow,
);

// Home area window glue: the Dashboard's Assigned to me and Recent canvases cards (`space_area_home`).
space_area_home::wire_home!(
    AppShell,
    PageLoadState,
    HomeState,
    HomeIssueRow,
    HomeCanvasRow,
);

// Projects area window glue: Create project, Overview, Issues, New issue, New sprint and the Issue page (`space_area_projects`).
space_area_projects::wire_projects!(
    AppShell,
    PageLoadState,
    SelectOption,
    MultiOption,
    CalendarDay,
    IssueRow,
    BoardChip,
    BoardItem,
    BoardLane,
    OverviewCountRow,
    OverviewAreaRow,
    AreaChip,
    IssueDetailView,
    IssueRef,
    LinkRow,
    CommentRow,
    WorkRow,
    AttachmentRow,
    AttachmentItem,
    ImageViewerItem,
    crate::clipboard::write_link,
);

// Canvas area window glue: the canvas page, its pop-out window and the canvas lists (`space_area_canvas`).
space_area_canvas::wire_canvas!(
    AppShell,
    CanvasPopout,
    PageLoadState,
    CanvasState,
    CanvasItemView,
    CanvasGuide,
    CanvasMenuPanel,
    CanvasMenuRow,
    CanvasHierarchyRow,
    CanvasPeer,
    CanvasPeerBox,
    CanvasVideoControl,
    CanvasAssetRow,
    CanvasWindowItem,
    CanvasListState,
    TableColumn,
    TableRow,
    SelectOption,
);

// Content area window glue (CMS.6): the content pages (space_area_content).
space_area_content::wire_content!(
    AppShell,
    PageLoadState,
    ContentHomeState,
    ContentPageState,
    ContentCollectionsState,
    ContentMediaState,
    ContentReviewsState,
    ContentTabItem,
    ListRowItem,
    DraftRowItem,
    ChoiceItem,
    PageHeaderItem,
    CollectionRowItem,
    FieldRowItem,
    MediaCellItem,
    ReviewRowItem,
    PropertyItem,
    crate::clipboard::write_link
);

/// The shared block editor (CMS.5, rimelight-rs `rimelight_editor`) on the generated `AppShell` and its `EditorBridge` global:
/// `bind_block_editor` (the page's controller and host callback) and `BlockEditorBinding` (`refresh`, `set_remote_cursors`,
/// `swap_controller`).
pub mod block_editor {
    use crate::{
        AppShell, BlockMenuItem, CommentItem, EditorBridge, EditorRow, EmojiItem, InsertItem,
        InspectorField, KeyOutcome, LocaleItem, MentionItem, PresenceItem, RemoteCaret,
    };
    rimelight_editor::wire_block_editor!(
        AppShell,
        EditorBridge,
        EditorRow,
        RemoteCaret,
        InsertItem,
        MentionItem,
        InspectorField,
        KeyOutcome,
        EmojiItem,
        BlockMenuItem,
        LocaleItem,
        CommentItem,
        PresenceItem
    );
}
