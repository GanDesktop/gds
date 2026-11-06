use super::{DirectoryPicker, Entry, Loader, Location, Navigation, Page, Request};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, Render, TestAppContext, Window, WindowHandle, div,
    px, size,
};
use gpui_kit::{
    component::{Root, WindowExt},
    prelude::*,
    test::TestWindowExt,
};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use tokio::sync::oneshot;

struct Pending {
    request: Request,
    reply: oneshot::Sender<Result<Page, String>>,
}
struct Fixture {
    picker: Entity<DirectoryPicker>,
    window: WindowHandle<Root>,
    requests: Rc<RefCell<VecDeque<Pending>>>,
    accepted: Rc<RefCell<Vec<String>>>,
}
struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
fn fixture(cx: &mut TestAppContext) -> Fixture {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::i18n::install(crate::i18n::Language::English, cx).expect("catalog");
        crate::ui::dialog::register(cx);
    });
    let requests = Rc::new(RefCell::new(VecDeque::new()));
    let queued = requests.clone();
    let loader: Loader = Rc::new(move |request| {
        let (reply, receive) = oneshot::channel();
        queued.borrow_mut().push_back(Pending { request, reply });
        Box::pin(async move { receive.await.expect("source response") })
    });
    let accepted = Rc::new(RefCell::new(Vec::new()));
    let mut picker = None;
    let window = cx.open_window(size(px(720.), px(640.)), |window, cx| {
        let view = cx.new(|cx| DirectoryPicker::new(loader, None, window, cx));
        picker = Some(view);
        Root::new(cx.new(|_| Host), window, cx)
    });
    let picker = picker.expect("picker");
    let content = picker.clone();
    let choices = accepted.clone();
    cx.update_window(window.into(), |_, window, cx| {
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let confirm = content.clone();
            let cancel = content.clone();
            let choices = choices.clone();
            dialog
                .confirm()
                .width(px(640.))
                .title("Choose folder")
                .footer(
                    crate::ui::DialogActions::default()
                        .primary_disabled(content.read(cx).selected_path(cx).is_none()),
                )
                .child(content.clone())
                .on_ok(move |_, _, cx| {
                    if let Some(path) = confirm.read(cx).selected_path(cx) {
                        choices.borrow_mut().push(path);
                        true
                    } else {
                        false
                    }
                })
                .on_close(move |_, window, cx| {
                    cancel.update(cx, |picker, cx| picker.cancel_loading(window, cx))
                })
        });
    })
    .expect("dialog");
    Fixture {
        picker,
        window,
        requests,
        accepted,
    }
}
fn page(path: &str, names: &[&str]) -> Page {
    Page {
        location: Location {
            path: path.into(),
            label: path.rsplit('/').next().unwrap_or("/").into(),
        },
        parent: Some("/".into()),
        ancestors: vec![],
        entries: names
            .iter()
            .map(|name| Entry {
                location: Location {
                    path: format!("{path}/{name}"),
                    label: (*name).into(),
                },
                hidden: name.starts_with('.'),
            })
            .collect(),
        next_cursor: None,
    }
}
fn reply(f: &Fixture, value: Result<Page, String>, cx: &mut TestAppContext) {
    f.requests
        .borrow_mut()
        .pop_front()
        .expect("request")
        .reply
        .send(value)
        .expect("active request");
    cx.run_until_parked();
    cx.update_window(f.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.simulate_next_frame(cx);
    })
    .expect("frame");
    cx.run_until_parked();
}

#[gpui_kit::test]
fn navigation_keeps_history_and_cancellation_discards_in_flight_results(cx: &mut TestAppContext) {
    let f = fixture(cx);
    reply(&f, Ok(page("/home", &["one", "two"])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        f.picker.update(cx, |p, cx| {
            p.navigate(Some("/slow".into()), Navigation::Visit, window, cx)
        });
        assert!(f.picker.read(cx).selected_path(cx).is_none());
        f.picker.update(cx, |p, cx| {
            p.navigate(Some("/new".into()), Navigation::Visit, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    let stale = f.requests.borrow_mut().pop_front().unwrap();
    assert!(stale.reply.send(Ok(page("/slow", &[]))).is_err());
    reply(&f, Ok(page("/new", &[])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        assert_eq!(f.picker.read(cx).history.back, ["/home"]);
        f.picker.update(cx, |p, cx| p.back(window, cx));
    })
    .unwrap();
    assert_eq!(
        f.requests.borrow().front().unwrap().request.path.as_deref(),
        Some("/home")
    );
    reply(&f, Err("permission denied".into()), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        assert_eq!(
            f.picker.read(cx).page.as_ref().unwrap().location.path,
            "/new"
        );
        assert_eq!(f.picker.read(cx).history.back, ["/home"]);
        assert!(f.picker.read(cx).selected_path(cx).is_none());
        f.picker.update(cx, |p, cx| p.refresh(window, cx));
    })
    .unwrap();
    reply(&f, Ok(page("/home", &["one"])), cx);
    cx.update(|cx| {
        assert!(f.picker.read(cx).history.back.is_empty());
        assert_eq!(f.picker.read(cx).history.forward, ["/new"]);
    });
}

#[gpui_kit::test]
fn keyboard_selection_enters_a_folder_and_decision_footer_stays_available(cx: &mut TestAppContext) {
    let f = fixture(cx);
    reply(&f, Ok(page("/home", &["one", "two", ".hidden"])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(f.picker.read(cx).table.read(cx).delegate().rows.len(), 2);
        window.press("down", cx);
        window.render_frame(cx);
        assert_eq!(
            f.picker.read(cx).selected_path(cx).as_deref(),
            Some("/home/one")
        );
        assert!(window.find("dialog-confirm").visible());
        assert!(window.find("dialog-cancel").visible());
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        f.accepted.borrow().is_empty(),
        "Enter on the list navigates, not accepts"
    );
    assert_eq!(
        f.requests.borrow().front().unwrap().request.path.as_deref(),
        Some("/home/one")
    );
    reply(&f, Ok(page("/home/one", &[])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-l", cx);
        window.render_frame(cx);
        window.press("ctrl-a", cx);
        window.input("/typed", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(f.accepted.borrow().is_empty());
    assert_eq!(
        f.requests.borrow().front().unwrap().request.path.as_deref(),
        Some("/typed")
    );
    reply(&f, Ok(page("/typed", &[])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("dialog-confirm", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(*f.accepted.borrow(), ["/typed"]);
}

#[gpui_kit::test]
fn pagination_filtering_and_hidden_folders_preserve_source_paths(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let mut first = page("/remote", &["visible", ".secret"]);
    first.next_cursor = Some("next".into());
    reply(&f, Ok(first), cx);
    assert_eq!(
        f.requests
            .borrow()
            .front()
            .unwrap()
            .request
            .cursor
            .as_deref(),
        Some("next")
    );
    reply(&f, Ok(page("/remote", &["later"])), cx);
    cx.update_window(f.window.into(), |_, window, cx| {
        f.picker.update(cx, |p, cx| p.toggle_hidden(window, cx));
        assert_eq!(f.picker.read(cx).table.read(cx).delegate().rows.len(), 3);
        window.press("ctrl-f", cx);
        window.render_frame(cx);
        window.input("later", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(f.window.into(), |_, window, cx| {
        let p = f.picker.read(cx);
        assert_eq!(p.table.read(cx).delegate().rows.len(), 1);
        p.table
            .clone()
            .update(cx, |table, cx| table.set_selected_row(0, cx));
        assert_eq!(
            f.picker.read(cx).selected_path(cx).as_deref(),
            Some("/remote/later")
        );
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(f.accepted.borrow().is_empty());
}
