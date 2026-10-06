use std::path::{Path, PathBuf};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSImage, NSMenu,
    NSMenuItem, NSRunningApplication, NSScreen, NSStatusBar, NSStatusItem, NSView, NSWindow,
    NSWindowStyleMask, NSWorkspace, NSWorkspaceOpenConfiguration,
};
use objc2_foundation::{
    NSActivityOptions, NSAppleEventDescriptor, NSAppleEventManager, NSArray, NSBundle, NSError,
    NSPoint, NSProcessInfo, NSSize, NSString, NSURL,
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
    let running = !NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(bundle_id),
    )
    .is_empty();
    if running {
        // 起動中のChromiumは、引数付きで実行された2つ目のプロセスからURLとプロファイルを受け取って即終了させる。
        // LaunchServices経由 (open -na 相当) より速い
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

pub fn reveal_in_finder(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
}

pub fn open_file(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
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

pub fn show_picker(window: &NSWindow, size: (f64, f64)) {
    let mtm = window.mtm();
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.setLevel(POP_UP_MENU_WINDOW_LEVEL);
    window.setContentSize(NSSize::new(size.0, size.1));

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
    let (mut x, mut top) = (mouse.x - 16.0, mouse.y + 8.0);
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
}

pub fn show_settings(window: &NSWindow, size: (f64, f64)) {
    let mtm = window.mtm();
    window.setStyleMask(
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable,
    );
    window.setLevel(NORMAL_WINDOW_LEVEL);
    window.setTitle(&NSString::from_str("Lightwell 設定"));
    window.setContentSize(NSSize::new(size.0, size.1));
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
