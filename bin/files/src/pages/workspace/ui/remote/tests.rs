#[gpui_kit::test]
fn adding_a_network_location_keeps_the_network_page_and_loading_can_be_cancelled(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::*;
    use crate::features::connect_server::{Connected, Intent};
    use gpui_kit::component::Root;
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("workspace fixture");
    // Do not poll the runtime: this regression tests UI decisions without contacting any server.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::shared::i18n::install(gds::i18n::Language::English, cx).expect("translations");
        crate::features::file_operations::commands::register(cx);
    });
    let mut browser = None;
    let window = cx.open_window(size(px(1100.), px(740.)), |window, cx| {
        let view = cx.new(|cx| {
            Browser::new(
                LaunchOptions {
                    runtime: runtime.handle().clone(),
                    places: Ok(Places::default()),
                    start: directory.path().to_owned(),
                    home: directory.path().to_owned(),
                    preferences: configuration::Preferences::default(),
                    startup_error: None,
                    picker: None,
                },
                window,
                cx,
            )
        });
        browser = Some(view.clone());
        Root::new(view, window, cx)
    });
    let browser = browser.expect("workspace");
    cx.run_until_parked();
    let address = Address::parse("sftp://atlas@delta/home/atlas/test").expect("remote address");
    cx.update_window(window.into(), |_, window, cx| {
        browser.update(cx, |this, cx| {
            this.show_location(Location::Network, window, cx);
            this.add_network_location(window, cx);
            assert_eq!(this.tabs.len(), 1);
            assert!(!this.current_tab().expect("tab").navigation.is_loading());
            let tab = this.active_id;
            this.connector.update(cx, |_, cx| {
                cx.emit(Connected {
                    tab,
                    address: address.clone(),
                    intent: Intent::Add,
                })
            });
        });
    })
    .expect("add network location");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.close_all_dialogs(cx);
        browser.update(cx, |this, cx| {
            assert_eq!(this.tabs.len(), 1);
            assert_eq!(this.current_location(), Location::Network);
            assert!(this.places.is_favorite(Location::Remote(address.clone())));
            assert!(
                !this
                    .places
                    .recent
                    .contains(&Location::Remote(address.clone()))
            );
            let tab = this.active_id;
            let request = this
                .tab_slot_mut(tab)
                .expect("tab")
                .navigation
                .prepare_open(
                    Location::Remote(address.clone()),
                    None,
                    ReadOptions::default(),
                );
            assert_eq!(
                this.current_tab()
                    .expect("tab")
                    .navigation
                    .pending_location(),
                Some(&Location::Remote(address.clone()))
            );
            assert_eq!(this.current_location(), Location::Network);
            this.cancel_location_load(cx);
            assert!(!this.current_tab().expect("tab").navigation.is_loading());
            assert_eq!(
                this.tab_slot_mut(tab)
                    .expect("tab")
                    .navigation
                    .finish_load(&request, Ok(None))
                    .expect("late result"),
                LoadOutcome::Stale
            );
            assert_eq!(this.current_location(), Location::Network);
            this.open_location_tab(Location::Remote(address.clone()), false, Some(window), cx);
            assert_eq!(this.tabs.len(), 2);
            assert!(
                this.current_tab()
                    .expect("new remote tab")
                    .navigation
                    .is_loading()
            );
            this.cancel_location_load(cx);
            assert_eq!(this.current_location(), Location::Network);
            assert!(
                !this
                    .current_tab()
                    .expect("cancelled tab")
                    .navigation
                    .is_loading()
            );
            assert_eq!(this.tabs.len(), 2);
        });
    })
    .expect("saved position and cancelled navigation");
}
