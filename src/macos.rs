use std::cell::RefCell;
use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBitmapImageRep,
    NSColor, NSColorSpace, NSCompositingOperation, NSDeviceRGBColorSpace, NSEvent,
    NSEventModifierFlags, NSFont, NSFontWeightRegular, NSFontWeightSemibold, NSGraphicsContext,
    NSImage, NSMenu, NSMenuItem, NSOpenPanel, NSRunningApplication, NSScreen, NSStatusBar,
    NSStatusItem, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask, NSWindowTitleVisibility,
    NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_core_foundation::{CFNumber, CFRange, CFString, CFURL};
use objc2_core_text::{CTFont, kCTFontURLAttribute};
use objc2_foundation::{
    NSActivityOptions, NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSBundle, NSError,
    NSFileManager, NSPoint, NSProcessInfo, NSRect, NSSize, NSString, NSURL,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub const FALLBACK_BUNDLE_ID: &str = "tech.s2n.lightwell";

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
const INTERNET_EVENT_CLASS: u32 = fourcc(b"GURL");
const GET_URL: u32 = fourcc(b"GURL");
const CORE_EVENT_CLASS: u32 = fourcc(b"aevt");
const OPEN_DOCUMENTS: u32 = fourcc(b"odoc");
const KEY_DIRECT_OBJECT: u32 = fourcc(b"----");
const KEY_SENDER_PID: u32 = fourcc(b"spid");

pub enum Event {
    OpenUrl {
        url: String,
        sender_pid: Option<i32>,
    },
    OpenSettings,
    ReloadConfig,
    SetDefaultBrowser,
}

pub struct HandlerIvars {
    on_event: Box<dyn Fn(Event)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LightwellHandler"]
    #[ivars = HandlerIvars]
    pub struct Handler;

    impl Handler {
        #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
        fn handle_get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(url) = event
                .paramDescriptorForKeyword(KEY_DIRECT_OBJECT)
                .and_then(|d| d.stringValue())
            else {
                return;
            };
            self.emit(Event::OpenUrl {
                url: url.to_string(),
                sender_pid: sender_pid(event),
            });
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(
            &self,
            event: &NSAppleEventDescriptor,
            _reply: &NSAppleEventDescriptor,
        ) {
            let Some(list) = event.paramDescriptorForKeyword(KEY_DIRECT_OBJECT) else {
                return;
            };
            let pid = sender_pid(event);
            let items: Vec<_> = match list.numberOfItems() {
                0 => vec![list],
                n => (1..=n).filter_map(|i| list.descriptorAtIndex(i)).collect(),
            };
            for item in items {
                if let Some(url) = item.fileURLValue().and_then(|u| u.absoluteString()) {
                    self.emit(Event::OpenUrl {
                        url: url.to_string(),
                        sender_pid: pid,
                    });
                }
            }
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.emit(Event::OpenSettings);
        }

        #[unsafe(method(reloadConfig:))]
        fn reload_config(&self, _sender: Option<&AnyObject>) {
            self.emit(Event::ReloadConfig);
        }

        #[unsafe(method(setDefaultBrowser:))]
        fn set_default_browser(&self, _sender: Option<&AnyObject>) {
            self.emit(Event::SetDefaultBrowser);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl Handler {
    pub fn new(mtm: MainThreadMarker, on_event: impl Fn(Event) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandlerIvars {
            on_event: Box::new(on_event),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn emit(&self, event: Event) {
        (self.ivars().on_event)(event);
    }
}

fn sender_pid(event: &NSAppleEventDescriptor) -> Option<i32> {
    event
        .attributeDescriptorForKeyword(KEY_SENDER_PID)
        .map(|d| d.int32Value())
        .filter(|pid| *pid > 0)
}

/// NSApplication の起動処理が odoc ハンドラを上書きするため、起動前と起動後の両方で呼ぶ
pub fn install_apple_event_handlers(handler: &Handler) {
    let manager = NSAppleEventManager::sharedAppleEventManager();
    unsafe {
        manager.setEventHandler_andSelector_forEventClass_andEventID(
            handler,
            sel!(handleGetURLEvent:withReplyEvent:),
            INTERNET_EVENT_CLASS,
            GET_URL,
        );
        manager.setEventHandler_andSelector_forEventClass_andEventID(
            handler,
            sel!(handleOpenDocuments:withReplyEvent:),
            CORE_EVENT_CLASS,
            OPEN_DOCUMENTS,
        );
    }
}

pub fn install_status_item(handler: &Handler, mtm: MainThreadMarker) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    if let Some(button) = item.button(mtm) {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("arrow.triangle.branch"),
            Some(&NSString::from_str("Lightwell")),
        );
        if let Some(image) = image {
            image.setTemplate(true);
            button.setImage(Some(&image));
        } else {
            button.setTitle(&NSString::from_str("BR"));
        }
    }

    let menu = NSMenu::new(mtm);
    let add = |title: &str, action, key: &str| {
        let entry = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        unsafe { entry.setTarget(Some(handler)) };
        menu.addItem(&entry);
    };
    add("設定…", sel!(openSettings:), ",");
    add("設定ファイルを再読み込み", sel!(reloadConfig:), "");
    add("デフォルトブラウザに設定", sel!(setDefaultBrowser:), "");
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    add("終了", sel!(quit:), "q");
    item.setMenu(Some(&menu));
    item
}

/// 常駐中にApp Napで眠らされると、リンクを開いたときの応答が遅れる
pub fn disable_app_nap() {
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        &NSString::from_str("Routing links without delay"),
    );
    std::mem::forget(token);
}

pub fn own_bundle_id() -> String {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_else(|| FALLBACK_BUNDLE_ID.to_owned())
}

pub fn fn_pressed() -> bool {
    NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Function)
}

pub fn bundle_id_of_pid(pid: i32) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|s| s.to_string())
}

pub struct RunningApp {
    pub name: String,
    pub bundle_id: String,
}

pub fn running_apps() -> Vec<RunningApp> {
    let mut apps: Vec<RunningApp> = NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
        .filter_map(|app| {
            Some(RunningApp {
                name: app.localizedName()?.to_string(),
                bundle_id: app.bundleIdentifier()?.to_string(),
            })
        })
        .collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

pub fn app_path_for_bundle_id(bundle_id: &str) -> Option<PathBuf> {
    let url = NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    Some(PathBuf::from(url.path()?.to_string()))
}

pub fn open_url(
    app_path: &Path,
    bundle_id: &str,
    url: &str,
    profile: Option<&str>,
) -> Result<(), String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app_url = NSURL::fileURLWithPath(&NSString::from_str(&app_path.to_string_lossy()));
    let config = NSWorkspaceOpenConfiguration::configuration();

    let Some(profile) = profile else {
        let target = NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| format!("URLとして解釈できません: {url}"))?;
        workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
            &NSArray::from_retained_slice(&[target]),
            &app_url,
            &config,
            None,
        );
        return Ok(());
    };

    let profile_arg = format!("--profile-directory={profile}");
    if let Some(dir) = crate::browsers::chromium_user_data_dir(bundle_id)
        && notify_chromium_singleton(&dir, app_path, &[&profile_arg, url]).is_ok()
    {
        return Ok(());
    }
    let running = !NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(bundle_id),
    )
    .is_empty();
    if running {
        // ソケットに送れなかった場合の予備。LaunchServices経由 (open -na 相当) より速いが、
        // 巨大なFrameworkの読み込みで数秒かかることがある
        let executable = NSBundle::bundleWithURL(&app_url)
            .and_then(|b| b.executableURL())
            .and_then(|u| u.path())
            .ok_or("実行ファイルが見つかりません")?;
        let mut child = std::process::Command::new(executable.to_string())
            .arg(profile_arg)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        std::thread::spawn(move || child.wait());
    } else {
        // 未起動時に直接実行すると、ブラウザ本体がこのアプリの子プロセスになり、
        // カメラ等の権限ダイアログがこのアプリ名義で出てしまう
        config.setArguments(&NSArray::from_retained_slice(&[
            NSString::from_str(&profile_arg),
            NSString::from_str(url),
        ]));
        workspace.openApplicationAtURL_configuration_completionHandler(&app_url, &config, None);
    }
    Ok(())
}

/// 起動中のChromiumの SingletonSocket に、2つ目のプロセスが送るのと同じコマンドラインを直接渡す。
/// 2つ目のプロセスを起動すると更新直後やキャッシュ追い出し後に数秒かかるため。
/// ACKはUIスレッドの処理後に返るので、メインスレッドを止めないよう待たない
fn notify_chromium_singleton(
    user_data_dir: &Path,
    app_path: &Path,
    args: &[&str],
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;

    let mut msg = b"START\0/".to_vec();
    msg.push(0);
    msg.extend_from_slice(app_path.as_os_str().as_bytes());
    for arg in args {
        msg.push(0);
        msg.extend_from_slice(arg.as_bytes());
    }
    let mut stream =
        std::os::unix::net::UnixStream::connect(user_data_dir.join("SingletonSocket"))?;
    stream.write_all(&msg)?;
    stream.shutdown(std::net::Shutdown::Write)
}

pub fn reveal_in_finder(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
}

pub fn is_default_browser() -> bool {
    let probe = NSURL::URLWithString(&NSString::from_str("https://example.com")).unwrap();
    NSWorkspace::sharedWorkspace()
        .URLForApplicationToOpenURL(&probe)
        .and_then(|url| NSBundle::bundleWithURL(&url))
        .and_then(|b| b.bundleIdentifier())
        .is_some_and(|id| id.to_string() == own_bundle_id())
}

pub fn set_as_default_browser() {
    let workspace = NSWorkspace::sharedWorkspace();
    let own = NSBundle::mainBundle().bundleURL();
    // 確認ダイアログが同時に複数出ると片方が失敗するので、httpの完了を待ってからhttpsを設定する
    let then_https = RcBlock::new(move |_err: *mut NSError| {
        NSWorkspace::sharedWorkspace()
            .setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
                &NSBundle::mainBundle().bundleURL(),
                &NSString::from_str("https"),
                None,
            );
    });
    workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
        &own,
        &NSString::from_str("http"),
        Some(&then_https),
    );
}

pub fn ns_window(handle: &impl HasWindowHandle) -> Option<Retained<NSWindow>> {
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::AppKit(h) => {
            let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
            view.window()
        }
        _ => None,
    }
}

const NORMAL_WINDOW_LEVEL: isize = 0;
const POP_UP_MENU_WINDOW_LEVEL: isize = 101;
const PICKER_CORNER_RADIUS: f64 = 14.0;

thread_local! {
    static BACKDROP: RefCell<Option<Retained<NSVisualEffectView>>> = const { RefCell::new(None) };
}

/// GLで描くeguiの背面に、すりガラス (NSVisualEffectView) を敷く。
/// contentView (winitのビュー) の子にするとGLの描画を覆ってしまうので、contentView の兄弟として背面に置く。
/// styleMask を変えると親のフレームビューが差し替わるので、その都度付け直す
fn install_backdrop(window: &NSWindow, material: NSVisualEffectMaterial, corner_radius: f64) {
    let Some(content) = window.contentView() else {
        return;
    };
    let Some(frame_view) = (unsafe { content.superview() }) else {
        return;
    };
    let mtm = window.mtm();
    let effect = BACKDROP.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| {
            let effect =
                NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), NSRect::ZERO);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect.setState(NSVisualEffectState::Active);
            effect.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            effect.setWantsLayer(true);
            effect
        })
        .clone()
    });
    effect.removeFromSuperview();
    effect.setMaterial(material);
    effect.setFrame(frame_view.bounds());
    let layer: Option<Retained<AnyObject>> = unsafe { msg_send![&*effect, layer] };
    if let Some(layer) = layer {
        let _: () = unsafe { msg_send![&*layer, setCornerRadius: corner_radius] };
        let _: () = unsafe { msg_send![&*layer, setMasksToBounds: corner_radius > 0.0] };
    }
    frame_view.addSubview_positioned_relativeTo(
        &effect,
        NSWindowOrderingMode::Below,
        Some(&content),
    );
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
}

pub fn show_picker(window: &NSWindow, size: (f64, f64)) {
    let mtm = window.mtm();
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.setLevel(POP_UP_MENU_WINDOW_LEVEL);
    window.setHasShadow(true);
    window.setContentSize(NSSize::new(size.0, size.1));
    install_backdrop(window, NSVisualEffectMaterial::Menu, PICKER_CORNER_RADIUS);

    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens.iter().find(|s| {
        let f = s.frame();
        mouse.x >= f.origin.x
            && mouse.x <= f.origin.x + f.size.width
            && mouse.y >= f.origin.y
            && mouse.y <= f.origin.y + f.size.height
    });
    let frame = window.frame();
    let (mut x, mut top) = (mouse.x - frame.size.width / 2.0, mouse.y + 12.0);
    if let Some(screen) = screen {
        let v = screen.visibleFrame();
        x = x.clamp(
            v.origin.x,
            (v.origin.x + v.size.width - frame.size.width).max(v.origin.x),
        );
        top = top.clamp(v.origin.y + frame.size.height, v.origin.y + v.size.height);
    }
    window.setFrameTopLeftPoint(NSPoint::new(x, top));
    activate(mtm);
    window.setAlphaValue(1.0);
    window.makeKeyAndOrderFront(None);
    window.invalidateShadow();
}

pub fn show_settings(window: &NSWindow, size: (f64, f64), min_size: (f64, f64)) {
    let mtm = window.mtm();
    window.setStyleMask(
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView,
    );
    window.setTitlebarAppearsTransparent(true);
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setTitle(&NSString::from_str("Lightwell"));
    window.setLevel(NORMAL_WINDOW_LEVEL);
    window.setHasShadow(true);
    window.setContentMinSize(NSSize::new(min_size.0, min_size.1));
    window.setContentSize(NSSize::new(size.0, size.1));
    install_backdrop(window, NSVisualEffectMaterial::Sidebar, 0.0);
    window.center();
    activate(mtm);
    window.setAlphaValue(1.0);
    window.makeKeyAndOrderFront(None);
}

/// eframeは最初の描画後にウィンドウを強制的に表示するので、透明にしておいて表示直後に隠す
pub fn conceal_initial_window(window: &NSWindow) {
    window.setAlphaValue(0.0);
}

pub fn order_out(window: &NSWindow) {
    window.orderOut(None);
}

/// 選択UIを閉じた後、リンクを開いた元のアプリにフォーカスを戻すため hide する
pub fn hide(window: &NSWindow) {
    window.orderOut(None);
    NSApplication::sharedApplication(window.mtm()).hide(None);
}

#[allow(deprecated)]
fn activate(mtm: MainThreadMarker) {
    // activate() は macOS 14 以降にしか存在しない
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemStatus {
    Enabled,
    Disabled,
    RequiresApproval,
}

pub fn login_item_status() -> LoginItemStatus {
    match unsafe { SMAppService::mainAppService().status() } {
        SMAppServiceStatus::Enabled => LoginItemStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
        _ => LoginItemStatus::Disabled,
    }
}

pub fn set_login_item(enabled: bool) -> Result<(), String> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    result.map_err(|e| e.localizedDescription().to_string())
}

pub fn open_login_items_settings() {
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

pub fn accent_color() -> [u8; 3] {
    let fallback = [10, 130, 255];
    let Some(color) =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
    else {
        return fallback;
    };
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        c(color.redComponent()),
        c(color.greenComponent()),
        c(color.blueComponent()),
    ]
}

pub fn rasterize(image: &NSImage, px: usize) -> Option<Vec<u8>> {
    let size = px as isize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            size,
            size,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            size * 4,
            32,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    // アスペクト比を保ったまま中央に収める (SF Symbolsは正方形とは限らない)
    let src = image.size();
    let scale = (px as f64 / src.width.max(1.0)).min(px as f64 / src.height.max(1.0));
    let (w, h) = (src.width * scale, src.height * scale);
    let rect = NSRect::new(
        NSPoint::new((px as f64 - w) / 2.0, (px as f64 - h) / 2.0),
        NSSize::new(w, h),
    );
    image.drawInRect_fromRect_operation_fraction(
        rect,
        NSRect::ZERO,
        NSCompositingOperation::SourceOver,
        1.0,
    );
    ctx.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let len = (rep.bytesPerRow() * size) as usize;
    Some(unsafe { std::slice::from_raw_parts(data, len) }.to_vec())
}

pub fn app_icon(path: &Path, px: usize) -> Option<Vec<u8>> {
    let image =
        NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()));
    rasterize(&image, px)
}

/// SF Symbols を白で描いたRGBA。eguiの tint で好きな色にできる
pub fn symbol(name: &str, px: usize) -> Option<Vec<u8>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        None,
    )?;
    let mut rgba = rasterize(&image, px)?;
    for pixel in rgba.chunks_exact_mut(4) {
        let a = pixel[3];
        pixel[0] = a;
        pixel[1] = a;
        pixel[2] = a;
    }
    Some(rgba)
}

pub struct AppInfo {
    pub name: String,
    pub path: PathBuf,
}

pub fn app_info(bundle_id: &str) -> Option<AppInfo> {
    let path = app_path_for_bundle_id(bundle_id)?;
    let name = NSFileManager::defaultManager()
        .displayNameAtPath(&NSString::from_str(&path.to_string_lossy()))
        .to_string();
    Some(AppInfo {
        name: name.trim_end_matches(".app").to_owned(),
        path,
    })
}

#[allow(deprecated)]
pub fn choose_app(mtm: MainThreadMarker) -> Option<String> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    // setAllowedContentTypes は UniformTypeIdentifiers への依存が増えるだけなので使わない
    panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[NSString::from_str(
        "app",
    )])));
    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
        "/Applications",
    ))));
    activate(mtm);
    if panel.runModal() != 1 {
        return None;
    }
    let url = panel.URLs().firstObject()?;
    Some(
        NSBundle::bundleWithURL(&url)?
            .bundleIdentifier()?
            .to_string(),
    )
}

#[cfg(debug_assertions)]
pub fn force_light_appearance() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let appearance = objc2_app_kit::NSAppearance::appearanceNamed(unsafe {
        objc2_app_kit::NSAppearanceNameAqua
    });
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}

#[cfg(debug_assertions)]
pub fn window_number(window: &NSWindow) -> isize {
    window.windowNumber()
}

pub struct FontFace {
    pub bytes: Vec<u8>,
    pub index: u32,
    pub variation: Vec<([u8; 4], f32)>,
}

pub struct UiFonts {
    pub regular: Option<FontFace>,
    pub semibold: Option<FontFace>,
    pub japanese: Option<FontFace>,
    pub japanese_semibold: Option<FontFace>,
}

pub fn ui_fonts(size: f64) -> UiFonts {
    let regular = NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightRegular });
    let semibold = NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightSemibold });
    let (regular, semibold) = (as_ct_font(&regular), as_ct_font(&semibold));
    UiFonts {
        regular: font_face(regular),
        semibold: font_face(semibold),
        japanese: font_face(&fallback_for(regular, "あ", "ja")),
        japanese_semibold: font_face(&fallback_for(semibold, "あ", "ja")),
    }
}

fn as_ct_font(font: &NSFont) -> &CTFont {
    // NSFont と CTFont は toll-free bridged
    unsafe { &*(font as *const NSFont).cast::<CTFont>() }
}

fn fallback_for(
    font: &CTFont,
    text: &str,
    language: &str,
) -> objc2_core_foundation::CFRetained<CTFont> {
    let text = CFString::from_str(text);
    let range = CFRange {
        location: 0,
        length: text.length(),
    };
    unsafe { font.for_string_with_language(&text, range, Some(&CFString::from_str(language))) }
}

fn font_face(font: &CTFont) -> Option<FontFace> {
    let url = unsafe { font.attribute(kCTFontURLAttribute) }?
        .downcast::<CFURL>()
        .ok()?;
    let bytes = std::fs::read(url.to_file_path()?).ok()?;
    let postscript_name = unsafe { font.post_script_name() }.to_string();
    let index = face_index(&bytes, &postscript_name).unwrap_or(0);
    Some(FontFace {
        bytes,
        index,
        variation: variation(font),
    })
}

/// .ttc には複数の書体が入っているので、PostScript 名で目的の書体を探す
fn face_index(bytes: &[u8], postscript_name: &str) -> Option<u32> {
    use skrifa::MetadataProvider;
    let file = skrifa::raw::FileRef::new(bytes).ok()?;
    file.fonts()
        .position(|font| {
            font.ok()
                .and_then(|font| {
                    font.localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
                        .english_or_first()
                        .map(|name| name.to_string() == postscript_name)
                })
                .unwrap_or(false)
        })
        .map(|i| i as u32)
}

fn variation(font: &CTFont) -> Vec<([u8; 4], f32)> {
    let Some(dict) = (unsafe { font.variation() }) else {
        return Vec::new();
    };
    let dict = unsafe { dict.cast_unchecked::<CFNumber, objc2_core_foundation::CFType>() };
    let (keys, values) = dict.to_vecs();
    keys.iter()
        .zip(values)
        .filter_map(|(axis, value)| {
            let tag = (axis.as_i64()? as u32).to_be_bytes();
            let value = value.downcast::<CFNumber>().ok()?.as_f64()? as f32;
            Some((tag, value))
        })
        .collect()
}
