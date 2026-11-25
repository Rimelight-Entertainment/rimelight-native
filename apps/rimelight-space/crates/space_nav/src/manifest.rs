//! The manifest: every sidebar section and every route of PLAN 6.4, as data (P0.20).
//!
//! Section ids group the pages in the contextual sidebar. A route is listed once. The owning phase is the PLAN 12
//! phase that builds the page for real. The phase is a planning judgement made here from PLAN 12 and 5; it is not
//! in PLAN 6.4 itself, so the owner should check it when P0.20 is closed.

use std::sync::OnceLock;

use crate::{Area, Page, Section, Template};

use Template as T;

const SECTIONS: &[Section] = &[
    // Home. Mail and Calendar are rows under Personal (owner direction 5).
    Section {
        id: "home.personal",
        area: Area::Home,
        title: "Personal",
    },
    Section {
        id: "home.performance",
        area: Area::Home,
        title: "Performance",
    },
    Section {
        id: "home.time",
        area: Area::Home,
        title: "Time & Pay",
    },
    // Chat (owner redefinition, Teams-style). The sidebar's conversation rows are live records fed by the UI, so the
    // only static row is the landing page.
    Section {
        id: "chat.main",
        area: Area::Chat,
        title: "Chat",
    },
    // Projects
    Section {
        id: "projects.project",
        area: Area::Projects,
        title: "Project",
    },
    Section {
        id: "projects.management",
        area: Area::Projects,
        title: "Management",
    },
    Section {
        id: "projects.docs",
        area: Area::Projects,
        title: "Docs and files",
    },
    Section {
        id: "projects.code",
        area: Area::Projects,
        title: "Code and assets",
    },
    Section {
        id: "projects.admin",
        area: Area::Projects,
        title: "Team and equipment",
    },
    // Company
    Section {
        id: "company.org",
        area: Area::Company,
        title: "Organisation",
    },
    Section {
        id: "company.information",
        area: Area::Company,
        title: "Information",
    },
    // People
    Section {
        id: "people.overview",
        area: Area::People,
        title: "Overview",
    },
    Section {
        id: "people.time",
        area: Area::People,
        title: "Time off and attendance",
    },
    Section {
        id: "people.recruiting",
        area: Area::People,
        title: "Recruiting",
    },
    Section {
        id: "people.pay",
        area: Area::People,
        title: "Pay and benefits",
    },
    Section {
        id: "people.records",
        area: Area::People,
        title: "HR records",
    },
    // Finance
    Section {
        id: "finance.overview",
        area: Area::Finance,
        title: "Overview",
    },
    Section {
        id: "finance.ledger",
        area: Area::Finance,
        title: "Ledger",
    },
    Section {
        id: "finance.banking",
        area: Area::Finance,
        title: "Banking",
    },
    Section {
        id: "finance.receivables",
        area: Area::Finance,
        title: "Receivables",
    },
    Section {
        id: "finance.payables",
        area: Area::Finance,
        title: "Payables",
    },
    Section {
        id: "finance.planning",
        area: Area::Finance,
        title: "Planning",
    },
    Section {
        id: "finance.revenue",
        area: Area::Finance,
        title: "Revenue",
    },
    Section {
        id: "finance.tax",
        area: Area::Finance,
        title: "Tax and reporting",
    },
    Section {
        id: "finance.settings",
        area: Area::Finance,
        title: "Settings",
    },
    // Customers
    Section {
        id: "customers.helpdesk",
        area: Area::Customers,
        title: "Helpdesk",
    },
    Section {
        id: "customers.players",
        area: Area::Customers,
        title: "Players",
    },
    Section {
        id: "customers.sales",
        area: Area::Customers,
        title: "Sales",
    },
    Section {
        id: "customers.moderation",
        area: Area::Customers,
        title: "Moderation",
    },
    Section {
        id: "customers.settings",
        area: Area::Customers,
        title: "Settings",
    },
    // Operations
    Section {
        id: "ops.service",
        area: Area::Operations,
        title: "Service desk",
    },
    Section {
        id: "ops.inventory",
        area: Area::Operations,
        title: "Inventory",
    },
    Section {
        id: "ops.it",
        area: Area::Operations,
        title: "Devices and access",
    },
    Section {
        id: "ops.automation",
        area: Area::Operations,
        title: "Approvals and automation",
    },
    // Analytics
    Section {
        id: "analytics.main",
        area: Area::Analytics,
        title: "Dashboards and reports",
    },
    Section {
        id: "analytics.data",
        area: Area::Analytics,
        title: "Data",
    },
    // Admin
    Section {
        id: "admin.org",
        area: Area::Admin,
        title: "Organisation",
    },
    Section {
        id: "admin.security",
        area: Area::Admin,
        title: "Security and audit",
    },
    Section {
        id: "admin.integrations",
        area: Area::Admin,
        title: "Integrations",
    },
    Section {
        id: "admin.projects",
        area: Area::Admin,
        title: "Projects",
    },
    Section {
        id: "admin.content",
        area: Area::Admin,
        title: "Content",
    },
    Section {
        id: "admin.system",
        area: Area::Admin,
        title: "System",
    },
];

/// One route row: (route, title, templates, owning phase, sidebar icon). The section is given per group below.
type Row = (
    &'static str,
    &'static str,
    &'static [Template],
    u8,
    &'static str,
);

/// Home > Personal (owner direction 5): Dashboard, Mail, Calendar, Tasks, Notes, Canvases, in that order. Mail is one general page (`/mail`), and Calendar is `/calendar`; neither has sub-routes.
const HOME_PERSONAL: &[Row] = &[
    ("/home", "Dashboard", &[T::Dash], 2, "layout-dashboard"),
    ("/mail", "Mail", &[T::Split], 4, "mail"),
    ("/calendar", "Calendar", &[T::Cal], 2, "calendar-days"),
    ("/home/todos", "Tasks", &[T::List], 2, "list-checks"),
    (
        "/home/notes",
        "Notes",
        &[T::Split, T::Doc],
        2,
        "notebook-pen",
    ),
    ("/home/notes/:id", "Note", &[T::Doc], 2, "file-pen-line"),
    ("/home/reviews", "Reviews", &[T::List], 2, "clipboard-check"),
    ("/home/canvases", "Canvases", &[T::List], 2, "presentation"),
    (
        "/home/canvases/:id",
        "Canvas",
        &[T::Canvas],
        2,
        "presentation",
    ),
];

/// Home > Time & Pay (owner direction 5). Payslips keep the existing route and row (no duplicate route).
const HOME_TIME: &[Row] = &[
    ("/home/time", "Timesheet", &[T::Grid], 2, "clock"),
    (
        "/people/payroll/payslips",
        "Payslips",
        &[T::List],
        8,
        "receipt",
    ),
];

/// Home > Performance: Metrics and Feedbacks, each a page of its own.
const HOME_PERFORMANCE: &[Row] = &[
    (
        "/home/performance/metrics",
        "Metrics",
        &[T::Dash],
        2,
        "gauge",
    ),
    (
        "/home/performance/feedbacks",
        "Feedbacks",
        &[T::List],
        2,
        "megaphone",
    ),
];

/// Chat (owner redefinition): the landing page is the only sidebar row. The three record routes have parameters, so
/// they are not sidebar rows; the UI lists the live conversations under the Chat section.
const CHAT: &[Row] = &[
    ("/chat", "Chat", &[T::Chat], 3, "message-square"),
    ("/chat/channels/:id", "Channel", &[T::Chat], 3, "hash"),
    (
        "/chat/chats/:id",
        "Chat with one person",
        &[T::Chat],
        3,
        "user-round",
    ),
    ("/chat/rooms/:id", "Room", &[T::Chat], 3, "door-open"),
];

const PROJECT_MANAGEMENT: &[Row] = &[
    ("/p/:key/members", "Members", &[T::List], 5, "users"),
    ("/p/:key/settings", "Settings", &[T::Form], 5, "settings"),
];

const PROJECT_PAGES: &[Row] = &[
    ("/p/:key", "Overview", &[T::Dash], 5, "layout-dashboard"),
    ("/p/:key/issues", "Issues", &[T::List], 5, "circle-dot"),
    ("/p/:key/issues/:id", "Issue", &[T::Detail], 5, "ticket"),
    // Canvases (owner, 2026-11-19): right after Issues in the project sidebar.
    (
        "/p/:key/canvases",
        "Canvases",
        &[T::List],
        6,
        "presentation",
    ),
    (
        "/p/:key/canvases/:id",
        "Canvas",
        &[T::Canvas],
        6,
        "presentation",
    ),
    (
        "/p/:key/timeline",
        "Timeline",
        &[T::Timeline],
        5,
        "gantt-chart",
    ),
    (
        "/p/:key/calendar",
        "Project calendar",
        &[T::Cal],
        5,
        "calendar-days",
    ),
    (
        "/p/:key/roadmap",
        "Epics, releases and milestones",
        &[T::Timeline],
        5,
        "map",
    ),
    ("/p/:key/goals", "Project goals", &[T::Tree], 5, "target"),
    ("/p/:key/reports", "Reports", &[T::Dash], 5, "chart-column"),
    ("/projects/goals", "Goals and OKRs", &[T::Tree], 5, "target"),
    (
        "/projects/workload",
        "Workload and capacity",
        &[T::Timeline],
        5,
        "gauge",
    ),
];

const PROJECT_DOCS: &[Row] = &[
    ("/p/:key/docs", "Content", &[T::Doc], 6, "file-text"),
    (
        "/p/:key/docs/:page",
        "Doc page",
        &[T::Doc],
        6,
        "file-pen-line",
    ),
    ("/p/:key/databases", "Databases", &[T::List], 6, "database"),
    (
        "/p/:key/databases/:id",
        "Database",
        &[T::List],
        6,
        "table-properties",
    ),
    ("/p/:key/files", "Files", &[T::List], 5, "folder"),
];

const PROJECT_CODE: &[Row] = &[
    ("/p/:key/assets", "Asset pipeline", &[T::List], 7, "layers"),
    ("/p/:key/assets/:id", "Asset", &[T::Detail], 7, "box"),
    (
        "/p/:key/code/repos",
        "Repositories",
        &[T::List],
        7,
        "git-branch",
    ),
    (
        "/p/:key/code/reviews",
        "Code reviews",
        &[T::List],
        7,
        "git-pull-request",
    ),
    (
        "/p/:key/code/reviews/:id",
        "Code review",
        &[T::Review],
        7,
        "git-merge",
    ),
    ("/p/:key/code/builds", "Pipelines", &[T::List], 7, "play"),
    ("/p/:key/code/releases", "Releases", &[T::List], 7, "rocket"),
];

const PROJECT_ADMIN: &[Row] = &[(
    "/p/:key/inventory",
    "Allocated equipment",
    &[T::List],
    9,
    "package",
)];

/// Company > Organisation (owner list): Locations, Departments, Teams, Employees, in that order. Teams holds the org
/// chart (no separate route). A location's detail page holds its rooms and org meetings (no separate rows or routes).
/// Employees is the people-finder for any employee to view colleagues (owner decision, SF.3). It is not the HR system of record, which lives in People > HR records (`/people/employee-records`, HR roles only).
const COMPANY_ORG: &[Row] = &[
    ("/company/locations", "Locations", &[T::List], 8, "map-pin"),
    (
        "/company/locations/:location",
        "Location",
        &[T::Detail],
        8,
        "map-pin",
    ),
    (
        "/company/departments",
        "Departments",
        &[T::Tree],
        8,
        "building-2",
    ),
    ("/company/teams", "Teams", &[T::List, T::Tree], 8, "users"),
    ("/company/teams/:team", "Team", &[T::Detail], 8, "users"),
    (
        "/company/teams/:team/docs",
        "Team docs",
        &[T::Doc],
        8,
        "file-text",
    ),
    (
        "/company/teams/:team/docs/:page",
        "Team doc page",
        &[T::Doc],
        8,
        "file-pen-line",
    ),
    (
        "/company/departments/:dept/docs",
        "Department docs",
        &[T::Doc],
        8,
        "file-text",
    ),
    (
        "/company/departments/:dept/docs/:page",
        "Department doc page",
        &[T::Doc],
        8,
        "file-pen-line",
    ),
    ("/company/employees", "Employees", &[T::List], 8, "id-card"),
    (
        "/company/employees/:user",
        "Profile",
        &[T::Detail],
        8,
        "user-round",
    ),
];

/// Company > Information (owner list): Knowledge Base, Learning.
const COMPANY_INFORMATION: &[Row] = &[
    (
        "/company/knowledge",
        "Knowledge Base",
        &[T::Doc],
        8,
        "book-open",
    ),
    (
        "/company/knowledge/:page",
        "Knowledge page",
        &[T::Doc],
        8,
        "book-marked",
    ),
    (
        "/company/learning",
        "Learning",
        &[T::List],
        8,
        "graduation-cap",
    ),
    (
        "/company/learning/:course",
        "Course",
        &[T::Doc],
        8,
        "school",
    ),
];

const PEOPLE_OVERVIEW: &[Row] = &[
    ("/people", "HR dashboard", &[T::Dash], 8, "layout-dashboard"),
    (
        "/people/onboarding",
        "Onboarding",
        &[T::Board],
        8,
        "user-plus",
    ),
    (
        "/people/offboarding",
        "Offboarding",
        &[T::Board],
        8,
        "user-minus",
    ),
];

const PEOPLE_TIME: &[Row] = &[
    ("/people/time-off", "Time off", &[T::List], 8, "palm-tree"),
    (
        "/people/time-off/calendar",
        "Leave calendar",
        &[T::Cal],
        8,
        "calendar-range",
    ),
    (
        "/people/attendance",
        "Attendance and shifts",
        &[T::Timeline],
        8,
        "clock",
    ),
    (
        "/people/timesheets",
        "Timesheet approval",
        &[T::Grid],
        8,
        "clipboard-check",
    ),
];

const PEOPLE_RECRUITING: &[Row] = &[
    (
        "/people/recruiting/jobs",
        "Requisitions and postings",
        &[T::List],
        8,
        "briefcase-business",
    ),
    (
        "/people/recruiting/candidates",
        "Candidate pipeline",
        &[T::Board],
        8,
        "user-search",
    ),
    (
        "/people/recruiting/candidates/:id",
        "Candidate",
        &[T::Detail],
        8,
        "user-round",
    ),
    (
        "/people/recruiting/interviews",
        "Interviews and scorecards",
        &[T::List],
        8,
        "calendar-check",
    ),
    (
        "/people/recruiting/offers",
        "Offers",
        &[T::List],
        8,
        "handshake",
    ),
];

const PEOPLE_PAY: &[Row] = &[
    (
        "/people/payroll",
        "Payroll inputs and exports",
        &[T::List],
        8,
        "banknote",
    ),
    (
        "/people/expenses",
        "Expenses and cards",
        &[T::List],
        9,
        "credit-card",
    ),
    (
        "/people/benefits",
        "Benefits and pension",
        &[T::List],
        9,
        "heart-handshake",
    ),
    (
        "/people/equity",
        "Equity grants and vesting",
        &[T::List],
        9,
        "chart-pie",
    ),
    ("/people/policies", "Policies", &[T::Doc], 8, "scroll-text"),
    ("/people/settings", "HR settings", &[T::Form], 8, "settings"),
];

/// People > HR records (OQ34): HR roles only (see `Page::hr_only`). `contact` is not used by any other row, so it
/// does not repeat Company > Employees' `id-card`.
const PEOPLE_RECORDS: &[Row] = &[
    (
        "/people/employee-records",
        "Employee records",
        &[T::List],
        8,
        "contact",
    ),
    (
        "/people/employee-records/:employee",
        "Employee record",
        &[T::Detail],
        8,
        "contact",
    ),
];

const FINANCE_OVERVIEW: &[Row] = &[(
    "/finance",
    "Finance dashboard",
    &[T::Dash],
    10,
    "layout-dashboard",
)];

const FINANCE_LEDGER: &[Row] = &[
    (
        "/finance/ledger/accounts",
        "Chart of accounts",
        &[T::Tree],
        10,
        "book-marked",
    ),
    (
        "/finance/ledger/journals",
        "Journals",
        &[T::List],
        10,
        "notebook",
    ),
    (
        "/finance/ledger/entries",
        "Journal entries",
        &[T::List],
        10,
        "list",
    ),
    (
        "/finance/ledger/entries/:id",
        "Journal entry",
        &[T::Detail],
        10,
        "receipt-text",
    ),
];

const FINANCE_BANKING: &[Row] = &[
    (
        "/finance/banking/accounts",
        "Bank accounts and feeds",
        &[T::List],
        10,
        "landmark",
    ),
    (
        "/finance/banking/reconcile",
        "Reconciliation",
        &[T::Split],
        10,
        "scale",
    ),
];

const FINANCE_RECEIVABLES: &[Row] = &[
    (
        "/finance/receivables/customers",
        "Customers",
        &[T::List],
        10,
        "users",
    ),
    (
        "/finance/receivables/quotes",
        "Quotes",
        &[T::List],
        10,
        "file-pen",
    ),
    (
        "/finance/receivables/invoices",
        "Invoices",
        &[T::List],
        10,
        "file-spreadsheet",
    ),
    (
        "/finance/receivables/invoices/:id",
        "Invoice",
        &[T::Detail],
        10,
        "file-check",
    ),
    (
        "/finance/receivables/payments",
        "Payments and credit notes",
        &[T::List],
        10,
        "hand-coins",
    ),
];

const FINANCE_PAYABLES: &[Row] = &[
    (
        "/finance/payables/vendors",
        "Vendors",
        &[T::List],
        10,
        "truck",
    ),
    (
        "/finance/payables/requests",
        "Purchase requests",
        &[T::List],
        10,
        "shopping-cart",
    ),
    (
        "/finance/payables/orders",
        "Purchase orders",
        &[T::List],
        10,
        "package-check",
    ),
    (
        "/finance/payables/bills",
        "Bills",
        &[T::List],
        10,
        "receipt",
    ),
    (
        "/finance/payables/bills/:id",
        "Bill",
        &[T::Detail],
        10,
        "receipt-text",
    ),
    (
        "/finance/payables/payment-runs",
        "Payment runs",
        &[T::List],
        10,
        "send",
    ),
];

const FINANCE_PLANNING: &[Row] = &[
    (
        "/finance/budgets",
        "Budgets and forecasts",
        &[T::Grid],
        10,
        "wallet",
    ),
    (
        "/finance/cost-centres",
        "Cost centres",
        &[T::List],
        10,
        "pie-chart",
    ),
    (
        "/finance/assets",
        "Fixed assets and depreciation",
        &[T::List],
        10,
        "warehouse",
    ),
];

const FINANCE_REVENUE: &[Row] = &[
    (
        "/finance/revenue/sources",
        "Platform sources",
        &[T::List],
        10,
        "plug",
    ),
    (
        "/finance/revenue/statements",
        "Sales statements",
        &[T::List],
        10,
        "file-chart-column",
    ),
    (
        "/finance/revenue/royalties",
        "Royalties and revenue share",
        &[T::List],
        10,
        "percent",
    ),
];

const FINANCE_TAX: &[Row] = &[
    (
        "/finance/tax",
        "VAT returns and tax codes",
        &[T::List],
        10,
        "receipt-text",
    ),
    (
        "/finance/reports",
        "Financial reports",
        &[T::Dash],
        10,
        "file-chart-line",
    ),
    ("/finance/close", "Period close", &[T::List], 10, "lock"),
];

const FINANCE_SETTINGS: &[Row] = &[(
    "/finance/settings",
    "Finance settings",
    &[T::Form],
    10,
    "settings",
)];

const CUSTOMERS_HELPDESK: &[Row] = &[
    (
        "/customers/helpdesk",
        "Helpdesk tickets",
        &[T::Split],
        11,
        "headset",
    ),
    (
        "/customers/helpdesk/tickets/:id",
        "Ticket",
        &[T::Chat, T::Detail],
        11,
        "ticket",
    ),
    (
        "/customers/helpdesk/macros",
        "Macros",
        &[T::List],
        11,
        "zap",
    ),
    ("/customers/helpdesk/slas", "SLAs", &[T::List], 11, "timer"),
    (
        "/customers/helpdesk/reports",
        "Helpdesk reports",
        &[T::Dash],
        11,
        "chart-column",
    ),
];

const CUSTOMERS_PLAYERS: &[Row] = &[
    ("/customers/players", "Players", &[T::List], 11, "gamepad-2"),
    (
        "/customers/players/:id",
        "Player",
        &[T::Detail],
        11,
        "user-round",
    ),
];

const CUSTOMERS_SALES: &[Row] = &[
    ("/customers/contacts", "Contacts", &[T::List], 11, "contact"),
    (
        "/customers/companies",
        "Companies",
        &[T::List],
        11,
        "building",
    ),
    (
        "/customers/pipeline",
        "Sales pipeline",
        &[T::Board],
        11,
        "funnel",
    ),
    (
        "/customers/contracts",
        "Contracts and subscriptions",
        &[T::List],
        11,
        "file-signature",
    ),
];

const CUSTOMERS_MODERATION: &[Row] = &[
    (
        "/customers/moderation/queue",
        "Moderation queue",
        &[T::Split],
        11,
        "list-checks",
    ),
    (
        "/customers/moderation/reports",
        "Player reports",
        &[T::List],
        11,
        "flag",
    ),
    (
        "/customers/moderation/sanctions",
        "Sanctions",
        &[T::List],
        11,
        "ban",
    ),
    (
        "/customers/moderation/announcements",
        "Community announcements",
        &[T::List],
        11,
        "megaphone",
    ),
];

const CUSTOMERS_SETTINGS: &[Row] = &[(
    "/customers/settings",
    "Customer settings",
    &[T::Form],
    11,
    "settings",
)];

const OPS_SERVICE: &[Row] = &[
    (
        "/ops/service-desk",
        "Service desk",
        &[T::Split],
        9,
        "concierge-bell",
    ),
    (
        "/ops/service-desk/catalogue",
        "Request catalogue",
        &[T::List],
        9,
        "list",
    ),
];

const OPS_INVENTORY: &[Row] = &[
    ("/ops/inventory/items", "Items", &[T::List], 9, "package"),
    ("/ops/inventory/items/:id", "Item", &[T::Detail], 9, "box"),
    (
        "/ops/inventory/stock",
        "Stock levels",
        &[T::List],
        9,
        "layers",
    ),
    (
        "/ops/inventory/locations",
        "Warehouses and locations",
        &[T::Tree],
        9,
        "map-pin",
    ),
    (
        "/ops/inventory/moves",
        "Stock moves",
        &[T::List],
        9,
        "arrow-left-right",
    ),
    (
        "/ops/inventory/stocktakes",
        "Stocktakes",
        &[T::List],
        9,
        "clipboard-list",
    ),
];

const OPS_IT: &[Row] = &[
    ("/ops/devices", "Devices", &[T::List], 9, "laptop"),
    ("/ops/devices/:id", "Device", &[T::Detail], 9, "smartphone"),
    (
        "/ops/software",
        "Software and licences",
        &[T::List],
        9,
        "app-window",
    ),
    (
        "/ops/access",
        "App access and provisioning rules",
        &[T::List],
        9,
        "key-round",
    ),
    (
        "/ops/access/requests",
        "Access requests",
        &[T::List],
        9,
        "user-check",
    ),
];

const OPS_AUTOMATION: &[Row] = &[
    (
        "/ops/approvals",
        "Approvals and requests",
        &[T::List],
        8,
        "stamp",
    ),
    (
        "/ops/automations",
        "Automation rules",
        &[T::List],
        9,
        "workflow",
    ),
    (
        "/ops/automations/runs",
        "Run history",
        &[T::List],
        9,
        "history",
    ),
    (
        "/ops/automations/webhooks",
        "Webhooks",
        &[T::List],
        9,
        "webhook",
    ),
];

const ANALYTICS_MAIN: &[Row] = &[
    (
        "/analytics",
        "Dashboards",
        &[T::List],
        12,
        "layout-dashboard",
    ),
    (
        "/analytics/d/:id",
        "Dashboard",
        &[T::Dash],
        12,
        "layout-panel-top",
    ),
    (
        "/analytics/reports",
        "Reports",
        &[T::List],
        12,
        "file-chart-column",
    ),
    (
        "/analytics/reports/:id",
        "Report",
        &[T::Dash],
        12,
        "file-chart-line",
    ),
];

const ANALYTICS_DATA: &[Row] = &[(
    "/analytics/sources",
    "Data sources",
    &[T::List],
    12,
    "database",
)];

const ADMIN_ORG: &[Row] = &[
    (
        "/admin/organisation",
        "Organisation",
        &[T::Form],
        1,
        "building-2",
    ),
    (
        "/admin/users",
        "Users and guests",
        &[T::List],
        1,
        "user-cog",
    ),
    (
        "/admin/roles",
        "Roles and permission matrix",
        &[T::Grid],
        1,
        "shield-check",
    ),
    (
        "/admin/policies",
        "Access policies and policy tester",
        &[T::List],
        1,
        "scale",
    ),
    (
        "/admin/localisation",
        "Languages, formats and holidays",
        &[T::Form],
        1,
        "languages",
    ),
];

const ADMIN_SECURITY: &[Row] = &[
    (
        "/admin/security",
        "Sign-in, sessions and 2FA",
        &[T::Form],
        13,
        "lock",
    ),
    ("/admin/audit", "Audit log", &[T::List], 1, "scroll-text"),
    (
        "/admin/compliance",
        "Retention, exports and data requests",
        &[T::List],
        13,
        "file-check",
    ),
];

const ADMIN_INTEGRATIONS: &[Row] = &[
    (
        "/admin/integrations",
        "Integrations",
        &[T::List],
        7,
        "plug-zap",
    ),
    (
        "/admin/mail",
        "Mail domains and routing",
        &[T::List],
        4,
        "mail",
    ),
    (
        "/admin/chat",
        "Chat channels, guests and retention",
        &[T::List],
        3,
        "message-square",
    ),
];
const ADMIN_PROJECTS: &[Row] = &[("/admin/types-tags", "Types & Tags", &[T::Form], 1, "tag")];

/// Admin > Content (CMS.6): the content types (collections) and the media library.
const ADMIN_CONTENT: &[Row] = &[
    (
        "/admin/content/collections",
        "Collections",
        &[T::List],
        9,
        "layers",
    ),
    (
        "/admin/content/media",
        "Media library",
        &[T::List],
        9,
        "folders",
    ),
];

const ADMIN_SYSTEM: &[Row] = &[
    (
        "/admin/api",
        "API tokens and share links",
        &[T::List],
        1,
        "key-round",
    ),
    ("/admin/system", "System health", &[T::Dash], 1, "activity"),
];

/// Every group: the section id and the rows that belong to it, in manifest order.
const GROUPS: &[(&str, &[Row])] = &[
    ("home.personal", HOME_PERSONAL),
    ("home.performance", HOME_PERFORMANCE),
    ("home.time", HOME_TIME),
    ("chat.main", CHAT),
    ("projects.project", PROJECT_PAGES),
    ("projects.management", PROJECT_MANAGEMENT),
    ("projects.docs", PROJECT_DOCS),
    ("projects.code", PROJECT_CODE),
    ("projects.admin", PROJECT_ADMIN),
    ("company.org", COMPANY_ORG),
    ("company.information", COMPANY_INFORMATION),
    ("people.overview", PEOPLE_OVERVIEW),
    ("people.time", PEOPLE_TIME),
    ("people.recruiting", PEOPLE_RECRUITING),
    ("people.pay", PEOPLE_PAY),
    ("people.records", PEOPLE_RECORDS),
    ("finance.overview", FINANCE_OVERVIEW),
    ("finance.ledger", FINANCE_LEDGER),
    ("finance.banking", FINANCE_BANKING),
    ("finance.receivables", FINANCE_RECEIVABLES),
    ("finance.payables", FINANCE_PAYABLES),
    ("finance.planning", FINANCE_PLANNING),
    ("finance.revenue", FINANCE_REVENUE),
    ("finance.tax", FINANCE_TAX),
    ("finance.settings", FINANCE_SETTINGS),
    ("customers.helpdesk", CUSTOMERS_HELPDESK),
    ("customers.players", CUSTOMERS_PLAYERS),
    ("customers.sales", CUSTOMERS_SALES),
    ("customers.moderation", CUSTOMERS_MODERATION),
    ("customers.settings", CUSTOMERS_SETTINGS),
    ("ops.service", OPS_SERVICE),
    ("ops.inventory", OPS_INVENTORY),
    ("ops.it", OPS_IT),
    ("ops.automation", OPS_AUTOMATION),
    ("analytics.main", ANALYTICS_MAIN),
    ("analytics.data", ANALYTICS_DATA),
    ("admin.org", ADMIN_ORG),
    ("admin.security", ADMIN_SECURITY),
    ("admin.integrations", ADMIN_INTEGRATIONS),
    ("admin.projects", ADMIN_PROJECTS),
    ("admin.content", ADMIN_CONTENT),
    ("admin.system", ADMIN_SYSTEM),
];

/// Every section of the manifest, in sidebar order within each area.
pub fn sections() -> &'static [Section] {
    SECTIONS
}

/// The command palette search aliases of a route (OQ6). Only the pages the owner named have one.
fn aliases_for(route: &str) -> &'static [&'static str] {
    match route {
        "/p/:key/code/builds" => &["Builds", "Jobs"],
        _ => &[],
    }
}

/// Every page (route) of the manifest, built once from the groups above.
pub fn pages() -> &'static [Page] {
    static PAGES: OnceLock<Vec<Page>> = OnceLock::new();
    PAGES
        .get_or_init(|| {
            let mut out = Vec::new();
            for &(section_id, rows) in GROUPS {
                let s = SECTIONS
                    .iter()
                    .find(|s| s.id == section_id)
                    .unwrap_or_else(|| panic!("manifest group names unknown section {section_id}"));
                let (area, section) = (s.area, s.id);
                for &(route, title, templates, phase, icon) in rows {
                    out.push(Page {
                        route,
                        title,
                        area,
                        section,
                        templates,
                        phase,
                        icon,
                        primary_action: None,
                        aliases: aliases_for(route),
                    });
                }
            }
            out
        })
        .as_slice()
}
