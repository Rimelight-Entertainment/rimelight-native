//! Space's routes for the shared shell models (`rimelight_shell`, ST2.5b): the rail areas, the Home route and the page
//! titles, all read from the `space_nav` manifest. The models are generic over [`rimelight_shell::Routes`], and
//! Space names [`SpaceRoutes`] as their type parameter (see the type aliases in `tiles.rs`, `workspace.rs` and `app_shell.rs`).

use space_nav::Area;

/// The routes of the Space app: the `space_nav` manifest's pages and areas.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SpaceRoutes;

impl rimelight_shell::Routes for SpaceRoutes {
    type Area = Area;
    const HOME_ROUTE: &'static str = super::history::HOME_ROUTE;
    const HOME_AREA: Area = Area::Home;

    fn title(route: &str) -> Option<String> {
        space_nav::parse(route).map(|matched| matched.page.title.to_owned())
    }

    fn area_id(area: Area) -> &'static str {
        match area {
            Area::Home => "home",
            Area::Chat => "chat",
            Area::Projects => "projects",
            Area::Company => "company",
            Area::People => "people",
            Area::Finance => "finance",
            Area::Customers => "customers",
            Area::Operations => "operations",
            Area::Analytics => "analytics",
            Area::Admin => "admin",
        }
    }

    fn area_from_id(id: &str) -> Option<Area> {
        Area::ALL
            .iter()
            .copied()
            .find(|&area| <Self as rimelight_shell::Routes>::area_id(area) == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimelight_shell::Routes;

    #[test]
    fn every_area_round_trips_through_its_saved_id() {
        let mut ids = std::collections::HashSet::new();
        for area in Area::ALL {
            let id = SpaceRoutes::area_id(area);
            assert!(ids.insert(id), "{id} is used once");
            assert_eq!(SpaceRoutes::area_from_id(id), Some(area));
        }
        assert_eq!(SpaceRoutes::area_from_id("nowhere"), None);
    }

    #[test]
    fn home_route_has_the_manifest_title() {
        assert_eq!(
            SpaceRoutes::title(SpaceRoutes::HOME_ROUTE).as_deref(),
            space_nav::parse("/home").map(|matched| matched.page.title)
        );
        assert_eq!(SpaceRoutes::title("/no/such/page"), None);
    }
}
