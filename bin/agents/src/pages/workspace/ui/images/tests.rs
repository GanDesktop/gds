use std::sync::{Arc, Mutex};

use gpui_kit::base::TextViewState;
use gpui_kit::{
    DevicePixels, TestAppContext, WindowHandle,
    http_client::{FakeHttpClient, Response},
    size,
    test::TestWindowExt,
};

use super::*;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 96;

fn png() -> Vec<u8> {
    include_bytes!("../../../../shared/ui/assets/sample.png").to_vec()
}

struct Block {
    id: &'static str,
    cache: Entity<Cache>,
    markdown: Option<Entity<TextViewState>>,
    image: Option<bas::acp::ImageContent>,
    probe: Option<Probe>,
}

#[derive(Clone)]
struct Probe {
    source: Resource,
    frames: Arc<Mutex<Vec<ImageLoad>>>,
}

type ImageLoad = Option<Result<Arc<RenderImage>, ImageCacheError>>;

impl Block {
    fn with_probe(mut self, source: &str) -> Self {
        self.probe = Some(Probe {
            source: Resource::Uri(source.to_owned().into()),
            frames: Arc::default(),
        });
        self
    }
}

struct Fixture {
    blocks: Vec<Block>,
    _changed: Vec<Subscription>,
    hidden: bool,
}

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.hidden {
            return div();
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .children(self.blocks.iter().map(|block| {
                let content = if let Some(document) = &block.markdown {
                    wrap(
                        block.id,
                        &block.cache,
                        document,
                        markdown::view(document, cx).into_any_element(),
                        cx,
                    )
                } else {
                    acp_image(
                        block.id,
                        block.image.as_ref().expect("ACP image"),
                        &block.cache,
                        false,
                        cx,
                    )
                };
                let cache = block.cache.clone();
                let probe = block.probe.clone();
                div()
                    .on_children_prepainted(move |_, window, cx| {
                        if let Some(probe) = &probe {
                            let result =
                                cache.update(cx, |cache, cx| cache.load(&probe.source, window, cx));
                            probe.frames.lock().expect("image probes").push(result);
                        }
                    })
                    .id(block.id)
                    .test_support()
                    .w(px(300.))
                    .flex_shrink_0()
                    .child(content)
            }))
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.gds().tasks().init_test(false);
        theme::init_default(cx);
    });
}

fn window(cx: &mut TestAppContext, blocks: Vec<Block>) -> WindowHandle<Root> {
    fixture_window(cx, blocks).0
}

fn fixture_window(
    cx: &mut TestAppContext,
    blocks: Vec<Block>,
) -> (WindowHandle<Root>, Entity<Fixture>) {
    let mut entity = None;
    let window = cx.open_window(size(px(500.), px(900.)), |window, cx| {
        let fixture = cx.new(|cx| {
            let mut changed: Vec<_> = blocks
                .iter()
                .map(|block| cx.observe(&block.cache, |_, _, cx| cx.notify()))
                .collect();
            changed.extend(
                blocks
                    .iter()
                    .filter_map(|block| block.markdown.as_ref())
                    .map(|state| cx.observe(state, |_, _, cx| cx.notify())),
            );
            Fixture {
                blocks,
                _changed: changed,
                hidden: false,
            }
        });
        entity = Some(fixture.clone());
        Root::new(fixture, window, cx)
    });
    (window, entity.expect("window creates fixture"))
}

fn markdown_block(
    id: &'static str,
    source: String,
    directory: Option<PathBuf>,
    cx: &mut App,
) -> Block {
    Block {
        id,
        cache: Cache::new(directory, cx),
        markdown: Some(cx.new(|cx| TextViewState::markdown(&source, cx))),
        image: None,
        probe: None,
    }
}

fn assert_png(cache: &Entity<Cache>, probe: &Probe, cx: &App) -> Arc<RenderImage> {
    let image = probe
        .frames
        .lock()
        .expect("image probes")
        .last()
        .expect("prepaint probe ran")
        .clone()
        .expect("completed image load")
        .expect("decoded image");
    assert_eq!(
        image.size(0),
        size(DevicePixels(WIDTH as i32), DevicePixels(HEIGHT as i32))
    );
    assert_eq!(
        &image.as_bytes(0).expect("pixels")[..4],
        &[15, 70, 200, 255],
        "PNG colors remain intact"
    );
    assert!(
        cache.read(cx).errors.is_empty(),
        "real image loads without fallback"
    );
    image
}

#[test]
fn image_bytes_and_decoded_dimensions_are_bounded() {
    assert!(loading::normalized(&vec![0; loading::MAX_BYTES + 1]).is_err());
    let wide = image::DynamicImage::new_rgba8(4097, 1);
    let mut bytes = std::io::Cursor::new(Vec::new());
    wide.write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    assert!(loading::normalized(bytes.get_ref()).is_err());
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.get_ref())
    );
    assert!(decode_data_uri(&uri).is_err());
}

#[cfg(unix)]
#[test]
fn conversation_image_fifo_and_symlink_are_not_authorized() {
    let dir = tempfile::tempdir().expect("directory");
    let fifo = dir.path().join("image.png");
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).expect("FIFO");
    let link = dir.path().join("zero.png");
    std::os::unix::fs::symlink("/dev/zero", &link).expect("link");
    for path in [&fifo, &link] {
        let uri = url::Url::from_file_path(path).expect("file URL");
        assert!(resolve(&Resource::Uri(uri.as_str().to_owned().into()), None).is_err());
        assert!(resolve(&Resource::from(path.clone()), None).is_err());
    }
}

#[gpui_kit::test]
fn http_unknown_length_is_bounded_and_rejected_before_decode(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| {
        cx.set_http_client(FakeHttpClient::create(|_| async {
            let mut body = png();
            body.resize(loading::MAX_BYTES + 1, 0);
            Ok(Response::builder()
                .status(200)
                .body(body.into())
                .expect("response"))
        }))
    });
    let block = cx.update(|cx| {
        markdown_block(
            "oversize",
            "![image](https://example.invalid/big.png)".into(),
            None,
            cx,
        )
    });
    let cache = block.cache.clone();
    let window = window(cx, vec![block]);
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!cache.read(cx).errors.is_empty());
        assert!(
            cache
                .read(cx)
                .inline
                .values()
                .all(|entry| matches!(entry.result, Some(Err(_))))
        );
    })
    .expect("bounded HTTP");
}

#[gpui_kit::test]
fn image_cache_refuses_more_than_its_resource_budget(cx: &mut TestAppContext) {
    init(cx);
    let source = (0..=MAX_CACHED_IMAGES)
        .map(|index| format!("![image]({})", inline_uri(&index.to_string())))
        .collect::<Vec<_>>()
        .join(" ");
    let block = cx.update(|cx| markdown_block("budget", source, None, cx));
    let cache = block.cache.clone();
    let window = window(cx, vec![block]);
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(cache.read(cx).inline.len(), MAX_CACHED_IMAGES);
        assert_eq!(cache.read(cx).errors.len(), 1);
    })
    .expect("resource budget");
}

#[test]
fn conversation_images_reject_client_files_and_special_files() {
    for address in [
        "/dev/zero",
        "/dev/random",
        "file:///dev/zero",
        "/tmp/private.png",
        "relative.png",
    ] {
        assert!(
            resolve(
                &Resource::Uri(address.into()),
                Some(std::path::Path::new("/tmp"))
            )
            .is_err(),
            "untrusted image address: {address}"
        );
    }
}

#[gpui_kit::test]
fn markdown_local_paths_with_spaces_file_uri_and_relative_directory_are_denied(
    cx: &mut TestAppContext,
) {
    init(cx);
    let directory = tempfile::tempdir().expect("image directory");
    let path = directory
        .path()
        .join("Screenshot from 2026-10-10 14-22-58.png");
    std::fs::write(&path, png()).expect("local PNG");
    let absolute = path.to_string_lossy().into_owned();
    let file = url::Url::from_file_path(&path)
        .expect("file URI")
        .to_string();
    assert!(file.contains("%20"));
    let relative = path
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .into_owned();
    let sources = [absolute, file, relative];
    let blocks = cx.update(|cx| {
        ["absolute", "file", "relative"]
            .into_iter()
            .zip(&sources)
            .map(|(id, source)| {
                markdown_block(
                    id,
                    format!("![截图](<{source}>)"),
                    Some(directory.path().to_owned()),
                    cx,
                )
                .with_probe(source)
            })
            .collect::<Vec<_>>()
    });
    let caches: Vec<_> = blocks.iter().map(|block| block.cache.clone()).collect();
    let probes: Vec<_> = blocks
        .iter()
        .filter_map(|block| block.probe.clone())
        .collect();
    let window = window(cx, blocks);
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .expect("start local loads");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        for ((id, probe), cache) in ["absolute", "file", "relative"]
            .into_iter()
            .zip(&probes)
            .zip(&caches)
        {
            let _ = (id, probe);
            assert!(!cache.read(cx).errors.is_empty());
            assert!(cache.read(cx).inline.is_empty());
        }
    })
    .expect("local Markdown images");
}

#[gpui_kit::test]
fn acp_and_markdown_data_images_decode_once_and_failed_images_have_visible_fallbacks(
    cx: &mut TestAppContext,
) {
    init(cx);
    let encoded = base64::engine::general_purpose::STANDARD.encode(png());
    let uri = format!("data:image/png;base64,{encoded}");
    let bad_uri = "data:image/png;base64,broken";
    let blocks = cx.update(|cx| {
        vec![
            Block {
                id: "acp",
                cache: Cache::new(None, cx),
                markdown: None,
                image: Some(bas::acp::ImageContent::new(encoded, "image/png")),
                probe: None,
            }
            .with_probe(&uri),
            markdown_block("inline", format!("![photo]({uri})"), None, cx).with_probe(&uri),
            markdown_block("invalid", format!("![photo]({bad_uri})"), None, cx),
            markdown_block(
                "unsupported",
                "![photo](data:image/unknown;base64,AA==)".into(),
                None,
                cx,
            ),
        ]
    });
    let invalid_document = blocks[2].markdown.clone().expect("invalid image document");
    let caches: Vec<_> = blocks.iter().map(|block| block.cache.clone()).collect();
    let probes: Vec<_> = blocks
        .iter()
        .filter_map(|block| block.probe.clone())
        .collect();
    let window = window(cx, blocks);
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .expect("begin data loads");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .expect("decode data loads");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        for ((id, probe), cache) in ["acp", "inline"].into_iter().zip(&probes).zip(&caches) {
            let first = assert_png(cache, probe, cx);
            window.render_frame(cx);
            let second = assert_png(cache, probe, cx);
            assert!(Arc::ptr_eq(&first, &second), "decoded data is cached");
            assert!(window.find(id).bounds().size.height >= px(HEIGHT as f32));
        }
        assert_eq!(
            window.find("image-acp").bounds().size,
            size(px(WIDTH as f32), px(HEIGHT as f32))
        );
        for (id, cache) in ["invalid", "unsupported"].into_iter().zip(&caches[2..]) {
            let errors = &cache.read(cx).errors;
            assert_eq!(errors.len(), 1);
            for (key, message) in errors {
                assert!(message.starts_with("图片加载失败"));
                assert!(!message.contains("data:"));
                assert!(window.find(format!("image-error-{id}-{key}")).visible());
            }
            assert!(window.find(id).bounds().size.height >= px(32.));
        }
    })
    .expect("data images and failures");
    cx.update(|cx| {
        invalid_document.update(cx, |state, cx| state.set_text("图片已移除", cx));
    });
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| window.render_frame(cx))
        .expect("replace failed image with text");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            caches[2].read(cx).errors.is_empty(),
            "replaced image removes its error caption"
        );
        assert!(caches[2].read(cx).inline.is_empty());
        assert!(caches[2].read(cx).fallback.is_none());
        assert!(window.find("invalid").bounds().size.height < px(32.));
    })
    .expect("no obsolete failure");
}

fn inline_uri(parameter: &str) -> SharedUri {
    format!(
        "data:image/png;{parameter};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png())
    )
    .into()
}

#[gpui_kit::test]
fn embedded_images_never_bypass_the_conversation_cache(cx: &mut TestAppContext) {
    init(cx);
    let first = inline_uri("first-document");
    let second = inline_uri("second-document");
    // Probe the original bytes used by Base's default inline decoder, not our
    // normalized PNG (which has a different asset key).
    let source = gpui_kit::ImageSource::Image(Arc::new(Image::from_bytes(ImageFormat::Png, png())));
    let blocks = cx.update(|cx| {
        vec![
            markdown_block("first", format!("![image]({first})"), None, cx),
            markdown_block("second", format!("<img src=\"{second}\">"), None, cx),
        ]
    });
    let (window, fixture) = fixture_window(cx, blocks);
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(!source.is_asset_cached(cx));
    })
    .expect("native embedded image");
    for remaining in [1, 0] {
        cx.update(|cx| {
            fixture.update(cx, |fixture, cx| {
                fixture.blocks.truncate(remaining);
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
        })
        .expect("release document");
        cx.run_until_parked();
        cx.update(|cx| assert!(!source.is_asset_cached(cx)));
    }
}

#[gpui_kit::test]
fn content_replacement_cancels_obsolete_pending_loads_without_cancelling_shared_loads(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (old_sender, old_receiver) = futures::channel::oneshot::channel::<()>();
    let (stable_sender, stable_receiver) = futures::channel::oneshot::channel::<()>();
    let receivers = Arc::new(Mutex::new(HashMap::<_, _>::from_iter([
        ("/old.png".to_owned(), old_receiver),
        ("/stable.png".to_owned(), stable_receiver),
    ])));
    let requests = Arc::new(Mutex::new(Vec::new()));
    cx.update(|cx| {
        let requests = requests.clone();
        cx.set_http_client(FakeHttpClient::create(move |request| {
            let path = request.uri().path().to_owned();
            requests.lock().expect("requests").push(path.clone());
            let receiver = receivers.lock().expect("receivers").remove(&path);
            async move {
                receiver
                    .expect("one request per URI")
                    .await
                    .expect("response gate");
                Ok(Response::builder()
                    .status(200)
                    .body(png().into())
                    .expect("response"))
            }
        }));
    });
    let old = "https://example.invalid/old.png";
    let stable = "https://example.invalid/stable.png";
    let block = cx.update(|cx| {
        markdown_block(
            "pending",
            format!("<img src=\"{old}\"> ![stable]({stable})"),
            None,
            cx,
        )
    });
    let cache = block.cache.clone();
    let document = block.markdown.clone().expect("document");
    let window = window(cx, vec![block]);
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(cache.read(cx).inline.len(), 2));
    cx.update(|cx| {
        document.update(cx, |state, cx| {
            state.set_text(
                &format!("<img src=\"{stable}\"> ![again]({stable})\n\n<div>streaming"),
                cx,
            );
        })
    });
    cx.run_until_parked();
    assert!(
        old_sender.send(()).is_err(),
        "obsolete HTTP future was cancelled"
    );
    cx.update(|cx| assert_eq!(cache.read(cx).inline.len(), 1));
    stable_sender
        .send(())
        .expect("still referenced request survives");
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(cache.read(cx).errors.is_empty());
        let image = cache.read(cx).inline[&SharedUri::from(stable)]
            .result
            .clone();
        assert!(matches!(image, Some(Ok(_))));
        assert_eq!(requests.lock().expect("requests").len(), 2);
        // Start inline decoding and remove its owner before its task can run.
        let obsolete = inline_uri("pending");
        cache.update(cx, |cache, cx| {
            cache.start_inline(&obsolete, cx);
            assert!(cache.inline[&obsolete].result.is_none());
            cache.retain(
                HashSet::from_iter([Resource::Uri(stable.into())]),
                window,
                cx,
            );
            assert_eq!(cache.inline.len(), 1);
        });
    })
    .expect("shared request and cancelled inline task");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            cache.read(cx).inline.len() == 1
                && cache.read(cx).inline.contains_key(&SharedUri::from(stable)),
            "cancelled task cannot repopulate cache; stable resource remains"
        )
    });
}

#[test]
fn markdown_ownership_uses_the_parser_not_uri_substrings() {
    let source = r#"
![escaped](image\(one\).png)
![entity](image&#46;png)

`![code](not-an-image.png)`

[reference]: referenced.png
"#;
    let references = markdown_references(source).expect("GFM references");
    assert_eq!(
        references,
        HashSet::from_iter(["image(one).png", "image.png"].map(|uri| Resource::Uri(uri.into())))
    );
    assert!(
        markdown_references("images removed")
            .expect("plain text")
            .is_empty()
    );
    assert_eq!(
        markdown_references("<img src=\"image.png\">").expect("HTML image"),
        HashSet::from_iter([Resource::Uri("image.png".into())])
    );
}
