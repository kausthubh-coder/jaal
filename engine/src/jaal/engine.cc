#include "jaal/engine.h"

#include <cstddef>
#include <utility>

#include "base/check.h"
#include "base/functional/bind.h"
#include "base/logging.h"
#include "base/no_destructor.h"
#include "base/strings/utf_string_conversions.h"
#include "base/task/single_thread_task_runner.h"
#include "build/build_config.h"
#include "chrome/browser/tab_list/tab_removed_reason.h"
#include "chrome/browser/lifetime/application_lifetime_desktop.h"
#include "chrome/browser/profiles/profile_manager.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface.h"
#include "chrome/browser/ui/browser_window/public/create_browser_window.h"
#include "chrome/browser/ui/browser_window/public/global_browser_collection.h"
#include "chrome/browser/ui/navigator/browser_navigator.h"
#include "chrome/browser/ui/navigator/browser_navigator_params.h"
#include "chrome/browser/ui/tabs/tab_strip_model.h"
#include "components/keep_alive_registry/keep_alive_types.h"
#include "components/keep_alive_registry/scoped_keep_alive.h"
#include "components/tabs/public/tab_interface.h"
#include "content/public/browser/browser_thread.h"
#include "content/public/browser/navigation_controller.h"
#include "content/public/browser/reload_type.h"
#include "content/public/browser/web_contents.h"
#include "jaal/browser_window.h"
#include "ui/base/page_transition_types.h"
#include "ui/base/window_open_disposition.h"
#include "url/gurl.h"

#if BUILDFLAG(IS_WIN)
#include "ui/views/win/hwnd_util.h"
#endif

namespace jaal {

namespace {

// Every call must come from the UI thread (docs/engine.md › The C API, rule 1).
#define JAAL_CHECK_UI_THREAD() \
  DCHECK_CURRENTLY_ON(content::BrowserThread::UI)

// Marks the time the app spends inside one engine call.
class ScopedApiCall {
 public:
  ScopedApiCall() { Engine::Get().EnterApiCall(); }
  ScopedApiCall(const ScopedApiCall&) = delete;
  ScopedApiCall& operator=(const ScopedApiCall&) = delete;
  ~ScopedApiCall() { Engine::Get().LeaveApiCall(); }
};

}  // namespace

// static
Engine& Engine::Get() {
  static base::NoDestructor<Engine> engine;
  return *engine;
}

Engine::Engine() = default;
Engine::~Engine() = default;

void Engine::OnBrowserStarting() {
  keep_alive_ = std::make_unique<ScopedKeepAlive>(
      // jaal's stand-in for Chrome's Mac AppController, which is what holds
      // this origin in Chrome.
      KeepAliveOrigin::APP_CONTROLLER, KeepAliveRestartOption::DISABLED);
  closing_all_browsers_subscription_ = chrome::AddClosingAllBrowsersCallback(
      base::BindRepeating(&Engine::OnClosingAllBrowsers,
                          weak_factory_.GetWeakPtr()));
  browser_collection_observation_.Observe(GlobalBrowserCollection::GetInstance());
}

void Engine::StartUi(jaal_ui_main_fn ui_main) {
  JAAL_CHECK_UI_THREAD();
  // Posted first, so it reaches the app before anything its own calls cause.
  PostEvent({.kind = JAAL_EVENT_STARTED});
  ui_main(&Engine::GetApi);
}

void Engine::OnMainLoopEnded() {
  browser_collection_observation_.Reset();
  closing_all_browsers_subscription_ = {};
  sink_ = nullptr;
  sink_user_data_ = nullptr;
  shortcut_filter_ = nullptr;
  shortcut_filter_user_data_ = nullptr;
}

void Engine::OnWindowCreated(JaalBrowserWindow* window) {
  windows_.Add(window);
  window->browser()->GetTabStripModel()->AddObserver(this);
}

void Engine::OnWindowDestroyed(JaalBrowserWindow* window) {
  window->browser()->GetTabStripModel()->RemoveObserver(this);
  const jaal_handle handle = windows_.Remove(window);
  PostEvent({.kind = JAAL_EVENT_WINDOW_CLOSED, .window = handle});
}

void Engine::OnWindowStateChanged(JaalBrowserWindow* window) {
  const std::optional<jaal_handle> handle = windows_.Find(window);
  if (!handle) {
    return;
  }
  PendingEvent event = {.kind = JAAL_EVENT_WINDOW_STATE, .window = *handle};
  event.window_active = window->IsActive();
  event.window_state = window->GetState();
  event.client_width = window->GetClientSize().width();
  event.client_height = window->GetClientSize().height();
  event.scale = window->GetScale();
  PostEvent(std::move(event));
}

bool Engine::OnPageKeyDown(JaalBrowserWindow* window,
                           int key_code,
                           int blink_modifiers,
                           bool before_page) {
  // Blink's shift, control, alt and meta bits are JAAL_MOD_*'s.
  const uint32_t modifiers = static_cast<uint32_t>(blink_modifiers) & 0xFu;
  if (before_page &&
      (!shortcut_filter_ ||
       !shortcut_filter_(shortcut_filter_user_data_.get(),
                         static_cast<uint32_t>(key_code), modifiers))) {
    return false;
  }
  PendingEvent event = {.kind = JAAL_EVENT_KEY};
  event.window = windows_.Find(window).value_or(jaal_handle{0, 0});
  event.key_code = static_cast<uint32_t>(key_code);
  event.modifiers = modifiers;
  event.key_reserved = before_page;
  PostEvent(std::move(event));
  return before_page;
}

void Engine::OnTabStripModelChanged(TabStripModel* tab_strip_model,
                                    const TabStripModelChange& change,
                                    const TabStripSelectionChange& selection) {
  if (change.type() == TabStripModelChange::kInserted) {
    for (const auto& inserted : change.GetInsert()->contents) {
      const jaal_handle tab = tabs_.Add(inserted.tab);
      PendingEvent event = TabStateEvent(inserted.tab, {0, 0});
      event.kind = JAAL_EVENT_TAB_INSERTED;
      event.tab = tab;
      PostEvent(std::move(event));
    }
  } else if (change.type() == TabStripModelChange::kRemoved) {
    for (const auto& removed : change.GetRemove()->contents) {
      // A tab moving to another window keeps its handle; it arrives there as
      // an insert.
      if (removed.remove_reason ==
          TabRemovedReason::kInsertedIntoOtherTabStrip) {
        continue;
      }
      const jaal_handle tab = tabs_.Remove(removed.tab);
      PostEvent({.kind = JAAL_EVENT_TAB_REMOVED, .tab = tab});
    }
  }

  if (selection.active_tab_changed() && selection.new_tab) {
    PostEvent(TabStateEvent(selection.new_tab, {0, 0}));
  }
}

void Engine::OnTabChangedAt(tabs::TabInterface* tab,
                            TabChangeType change_type) {
  if (change_type == TabChangeType::kAll ||
      change_type == TabChangeType::kLoadingOnly) {
    PostEvent(TabStateEvent(tab, {0, 0}));
  }
}

void Engine::OnBrowserClosed(BrowserWindowInterface* browser) {
  MaybeFinishQuit();
}

Engine::PendingEvent Engine::TabStateEvent(tabs::TabInterface* tab,
                                           jaal_handle window) {
  PendingEvent event = {.kind = JAAL_EVENT_TAB_STATE};
  event.tab = tabs_.Find(tab).value_or(jaal_handle{0, 0});
  event.window = WindowHandleForTab(tab).value_or(window);
  content::WebContents* contents = tab->GetContents();
  event.url = contents->GetVisibleURL().spec();
  event.title = base::UTF16ToUTF8(contents->GetTitle());
  event.loading = contents->IsLoading();
  event.active = tab->IsActivated();
  event.can_go_back = contents->GetController().CanGoBack();
  event.can_go_forward = contents->GetController().CanGoForward();
  return event;
}

std::optional<jaal_handle> Engine::WindowHandleForTab(tabs::TabInterface* tab) {
  BrowserWindowInterface* browser = tab->GetBrowserWindowInterface();
  if (!browser) {
    return std::nullopt;
  }
  JaalBrowserWindow* window = JaalBrowserWindow::FromBrowser(browser);
  return window ? windows_.Find(window) : std::nullopt;
}

void Engine::PostEvent(PendingEvent event) {
  base::SingleThreadTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE, base::BindOnce(&Engine::DeliverEvent,
                                weak_factory_.GetWeakPtr(), std::move(event)));
}

void Engine::DeliverEvent(PendingEvent event) {
  if (!sink_) {
    return;
  }
  jaal_event out = {};
  out.struct_size = sizeof(jaal_event);
  out.kind = event.kind;
  out.window = event.window;
  out.tab = event.tab;
  out.url = event.url.empty() ? nullptr : event.url.c_str();
  out.title = event.title.empty() ? nullptr : event.title.c_str();
  out.loading = event.loading;
  out.active = event.active;
  out.key_code = event.key_code;
  out.modifiers = event.modifiers;
  out.key_reserved = event.key_reserved;
  out.window_active = event.window_active;
  out.window_state = event.window_state;
  out.client_width = event.client_width;
  out.client_height = event.client_height;
  out.scale = event.scale;
  out.can_go_back = event.can_go_back;
  out.can_go_forward = event.can_go_forward;
  sink_(sink_user_data_.get(), &out);
}

// Quitting ---------------------------------------------------------------

jaal_status Engine::Quit() {
  JAAL_CHECK_UI_THREAD();
  if (quitting_) {
    return JAAL_OK;
  }
  quitting_ = true;
  if (GlobalBrowserCollection::GetInstance()->IsEmpty()) {
    MaybeFinishQuit();
    return JAAL_OK;
  }
  // Runs every window's beforeunload checks; any of them can cancel, which
  // reaches OnClosingAllBrowsers(false).
  chrome::CloseAllBrowsersAndQuit();
  return JAAL_OK;
}

void Engine::OnClosingAllBrowsers(bool closing) {
  if (!closing && quitting_) {
    quitting_ = false;
    PostEvent({.kind = JAAL_EVENT_QUIT_CANCELLED});
  }
}

void Engine::MaybeFinishQuit() {
  if (!quitting_ || will_quit_sent_ ||
      !GlobalBrowserCollection::GetInstance()->IsEmpty()) {
    return;
  }
  // Every window closed, so every beforeunload passed: the quit is committed.
  will_quit_sent_ = true;
  PostEvent({.kind = JAAL_EVENT_WILL_QUIT});
  // Dropped after the event so the app hears it before the loop ends. With
  // the last keep-alive gone, Chrome ends its main loop.
  base::SingleThreadTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE, base::BindOnce(
                     [](base::WeakPtr<Engine> engine) {
                       if (engine) {
                         engine->keep_alive_.reset();
                       }
                     },
                     weak_factory_.GetWeakPtr()));
}

// The C API ---------------------------------------------------------------

jaal_status Engine::SetEventSink(jaal_event_sink sink, void* user_data) {
  JAAL_CHECK_UI_THREAD();
  sink_ = sink;
  sink_user_data_ = user_data;
  return JAAL_OK;
}

jaal_status Engine::WindowCreate(jaal_handle* out_window) {
  const jaal_window_options options = {.struct_size =
                                           sizeof(jaal_window_options)};
  return WindowCreateWith(&options, out_window);
}

jaal_status Engine::WindowCreateWith(const jaal_window_options* options,
                                     jaal_handle* out_window) {
  JAAL_CHECK_UI_THREAD();
  if (!out_window || !options ||
      options->struct_size < sizeof(jaal_window_options)) {
    return JAAL_ERR_INVALID;
  }
  if (quitting_) {
    return JAAL_ERR_CANCELLED;
  }
  Profile* profile = ProfileManager::GetLastUsedProfileAllowedByPolicy();
  if (!profile) {
    return JAAL_ERR_INTERNAL;
  }
  if (GetBrowserWindowCreationStatusForProfile(*profile) !=
      BrowserWindowInterface::CreationStatus::kOk) {
    return JAAL_ERR_DENIED;
  }
  creating_window_options_ = *options;
  BrowserWindowInterface* browser = CreateBrowserWindow(
      BrowserWindowCreateParams(*profile, /*from_user_gesture=*/true));
  creating_window_options_ = {};
  JaalBrowserWindow* window = JaalBrowserWindow::FromBrowser(browser);
  if (!window) {
    // The window hook isn't installed, so Chrome made its own window.
    return JAAL_ERR_INTERNAL;
  }
  window->Show();
  *out_window = windows_.Find(window).value();
  // The app lays its views out from this.
  OnWindowStateChanged(window);
  return JAAL_OK;
}

jaal_status Engine::WindowClose(jaal_handle handle) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  window->Close();
  return JAAL_OK;
}

jaal_status Engine::WindowNativeHandle(jaal_handle handle, void** out_handle) {
  JAAL_CHECK_UI_THREAD();
  if (!out_handle) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
#if BUILDFLAG(IS_WIN)
  *out_handle = views::HWNDForNativeWindow(window->GetNativeWindow());
  return JAAL_OK;
#else
  return JAAL_ERR_UNSUPPORTED;
#endif
}

jaal_status Engine::WindowSetPageRect(jaal_handle handle, jaal_rect rect) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  if (rect.width < 0 || rect.height < 0) {
    return JAAL_ERR_INVALID;
  }
  window->SetPageRect(gfx::Rect(rect.x, rect.y, rect.width, rect.height));
  return JAAL_OK;
}

jaal_status Engine::TabOpen(jaal_handle window_handle,
                            const char* url,
                            uint8_t foreground,
                            jaal_handle* out_tab) {
  JAAL_CHECK_UI_THREAD();
  if (!url || !out_tab) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(window_handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  const GURL gurl(url);
  if (!gurl.is_valid()) {
    return JAAL_ERR_INVALID;
  }
  NavigateParams params(window->browser(), gurl,
                        ui::PAGE_TRANSITION_AUTO_TOPLEVEL);
  params.disposition = foreground ? WindowOpenDisposition::NEW_FOREGROUND_TAB
                                  : WindowOpenDisposition::NEW_BACKGROUND_TAB;
  Navigate(&params);
  if (!params.navigated_or_inserted_contents) {
    return JAAL_ERR_INTERNAL;
  }
  tabs::TabInterface* tab = tabs::TabInterface::MaybeGetFromContents(
      params.navigated_or_inserted_contents);
  if (!tab) {
    return JAAL_ERR_INTERNAL;
  }
  // Normally the insert already registered it; Add() returns that handle.
  *out_tab = tabs_.Add(tab);
  return JAAL_OK;
}

jaal_status Engine::TabClose(jaal_handle handle) {
  JAAL_CHECK_UI_THREAD();
  tabs::TabInterface* tab = tabs_.Get(handle);
  if (!tab) {
    return JAAL_ERR_GONE;
  }
  tab->Close();
  return JAAL_OK;
}

jaal_status Engine::TabActivate(jaal_handle handle) {
  JAAL_CHECK_UI_THREAD();
  tabs::TabInterface* tab = tabs_.Get(handle);
  if (!tab) {
    return JAAL_ERR_GONE;
  }
  BrowserWindowInterface* browser = tab->GetBrowserWindowInterface();
  if (!browser) {
    return JAAL_ERR_GONE;
  }
  TabStripModel* model = browser->GetTabStripModel();
  model->ActivateTabAt(model->GetIndexOfTab(tab));
  return JAAL_OK;
}

jaal_status Engine::TabLoad(jaal_handle handle, const char* url) {
  JAAL_CHECK_UI_THREAD();
  if (!url) {
    return JAAL_ERR_INVALID;
  }
  tabs::TabInterface* tab = tabs_.Get(handle);
  if (!tab) {
    return JAAL_ERR_GONE;
  }
  const GURL gurl(url);
  if (!gurl.is_valid()) {
    return JAAL_ERR_INVALID;
  }
  content::NavigationController::LoadURLParams params(gurl);
  params.transition_type = ui::PAGE_TRANSITION_TYPED;
  tab->GetContents()->GetController().LoadURLWithParams(params);
  return JAAL_OK;
}

jaal_status Engine::SetShortcutFilter(jaal_shortcut_filter filter,
                                      void* user_data) {
  JAAL_CHECK_UI_THREAD();
  shortcut_filter_ = filter;
  shortcut_filter_user_data_ = user_data;
  return JAAL_OK;
}

jaal_status Engine::WindowSetPagePlacement(jaal_handle handle,
                                           jaal_placement placement) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  window->SetPagePlacement(placement);
  return JAAL_OK;
}

jaal_status Engine::WindowAttachView(jaal_handle handle,
                                     void* native_view,
                                     const jaal_view_options* options) {
  JAAL_CHECK_UI_THREAD();
  if (!native_view || !options ||
      options->struct_size < sizeof(jaal_view_options)) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(handle);
  return window ? window->AttachView(native_view, *options) : JAAL_ERR_GONE;
}

jaal_status Engine::WindowUpdateView(jaal_handle handle,
                                     void* native_view,
                                     const jaal_view_options* options) {
  JAAL_CHECK_UI_THREAD();
  if (!native_view || !options ||
      options->struct_size < sizeof(jaal_view_options)) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(handle);
  return window ? window->UpdateView(native_view, *options) : JAAL_ERR_GONE;
}

jaal_status Engine::WindowDetachView(jaal_handle handle, void* native_view) {
  JAAL_CHECK_UI_THREAD();
  if (!native_view) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(handle);
  return window ? window->DetachView(native_view) : JAAL_ERR_GONE;
}

jaal_status Engine::WindowFocusPage(jaal_handle handle) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  window->FocusPage();
  return JAAL_OK;
}

jaal_status Engine::WindowSetState(jaal_handle handle,
                                   jaal_window_state state) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  if (state < JAAL_WINDOW_NORMAL || state > JAAL_WINDOW_FULLSCREEN) {
    return JAAL_ERR_INVALID;
  }
  // Posted: a state change resizes the window at once, and the resize moves
  // the app's views, which must not happen while the app is inside this call.
  base::SingleThreadTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE, base::BindOnce(
                     [](base::WeakPtr<Engine> engine, jaal_handle handle,
                        jaal_window_state state) {
                       if (!engine) {
                         return;
                       }
                       if (JaalBrowserWindow* window =
                               engine->windows_.Get(handle)) {
                         window->SetState(state);
                       }
                     },
                     weak_factory_.GetWeakPtr(), handle, state));
  return JAAL_OK;
}

jaal_status Engine::WindowSetStyle(jaal_handle handle,
                                   const jaal_window_style* style) {
  JAAL_CHECK_UI_THREAD();
  if (!style || style->struct_size < sizeof(jaal_window_style)) {
    return JAAL_ERR_INVALID;
  }
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  window->SetStyle(*style);
  return JAAL_OK;
}

jaal_status Engine::TabNavigate(jaal_handle handle, jaal_nav_action action) {
  JAAL_CHECK_UI_THREAD();
  tabs::TabInterface* tab = tabs_.Get(handle);
  if (!tab) {
    return JAAL_ERR_GONE;
  }
  content::WebContents* contents = tab->GetContents();
  content::NavigationController& controller = contents->GetController();
  switch (action) {
    case JAAL_NAV_BACK:
      if (!controller.CanGoBack()) {
        return JAAL_ERR_INVALID;
      }
      controller.GoBack();
      return JAAL_OK;
    case JAAL_NAV_FORWARD:
      if (!controller.CanGoForward()) {
        return JAAL_ERR_INVALID;
      }
      controller.GoForward();
      return JAAL_OK;
    case JAAL_NAV_RELOAD:
      controller.Reload(content::ReloadType::NORMAL, /*check_for_repost=*/true);
      return JAAL_OK;
    case JAAL_NAV_STOP:
      contents->Stop();
      return JAAL_OK;
  }
  return JAAL_ERR_INVALID;
}

jaal_status Engine::WindowBeginDrag(jaal_handle handle) {
  JAAL_CHECK_UI_THREAD();
  JaalBrowserWindow* window = windows_.Get(handle);
  if (!window) {
    return JAAL_ERR_GONE;
  }
  window->BeginDrag();
  return JAAL_OK;
}

namespace {

jaal_status ApiSetEventSink(jaal_event_sink sink, void* user_data) {
  ScopedApiCall call;
  return Engine::Get().SetEventSink(sink, user_data);
}
jaal_status ApiQuit() {
  ScopedApiCall call;
  return Engine::Get().Quit();
}
jaal_status ApiWindowCreate(jaal_handle* out_window) {
  ScopedApiCall call;
  return Engine::Get().WindowCreate(out_window);
}
jaal_status ApiWindowClose(jaal_handle window) {
  ScopedApiCall call;
  return Engine::Get().WindowClose(window);
}
jaal_status ApiWindowNativeHandle(jaal_handle window, void** out_handle) {
  ScopedApiCall call;
  return Engine::Get().WindowNativeHandle(window, out_handle);
}
jaal_status ApiWindowSetPageRect(jaal_handle window, jaal_rect rect) {
  ScopedApiCall call;
  return Engine::Get().WindowSetPageRect(window, rect);
}
jaal_status ApiTabOpen(jaal_handle window,
                       const char* url,
                       uint8_t foreground,
                       jaal_handle* out_tab) {
  ScopedApiCall call;
  return Engine::Get().TabOpen(window, url, foreground, out_tab);
}
jaal_status ApiTabClose(jaal_handle tab) {
  ScopedApiCall call;
  return Engine::Get().TabClose(tab);
}
jaal_status ApiTabActivate(jaal_handle tab) {
  ScopedApiCall call;
  return Engine::Get().TabActivate(tab);
}
jaal_status ApiTabLoad(jaal_handle tab, const char* url) {
  ScopedApiCall call;
  return Engine::Get().TabLoad(tab, url);
}
jaal_status ApiSetShortcutFilter(jaal_shortcut_filter filter,
                                 void* user_data) {
  ScopedApiCall call;
  return Engine::Get().SetShortcutFilter(filter, user_data);
}
jaal_status ApiWindowSetPagePlacement(jaal_handle window,
                                      jaal_placement placement) {
  ScopedApiCall call;
  return Engine::Get().WindowSetPagePlacement(window, placement);
}
jaal_status ApiWindowAttachView(jaal_handle window,
                                void* native_view,
                                const jaal_view_options* options) {
  ScopedApiCall call;
  return Engine::Get().WindowAttachView(window, native_view, options);
}
jaal_status ApiWindowUpdateView(jaal_handle window,
                                void* native_view,
                                const jaal_view_options* options) {
  ScopedApiCall call;
  return Engine::Get().WindowUpdateView(window, native_view, options);
}
jaal_status ApiWindowDetachView(jaal_handle window, void* native_view) {
  ScopedApiCall call;
  return Engine::Get().WindowDetachView(window, native_view);
}
jaal_status ApiWindowFocusPage(jaal_handle window) {
  ScopedApiCall call;
  return Engine::Get().WindowFocusPage(window);
}
jaal_status ApiWindowSetState(jaal_handle window, jaal_window_state state) {
  ScopedApiCall call;
  return Engine::Get().WindowSetState(window, state);
}
jaal_status ApiWindowCreateWith(const jaal_window_options* options,
                                jaal_handle* out_window) {
  ScopedApiCall call;
  return Engine::Get().WindowCreateWith(options, out_window);
}
jaal_status ApiWindowSetStyle(jaal_handle window,
                              const jaal_window_style* style) {
  ScopedApiCall call;
  return Engine::Get().WindowSetStyle(window, style);
}
jaal_status ApiWindowBeginDrag(jaal_handle window) {
  ScopedApiCall call;
  return Engine::Get().WindowBeginDrag(window);
}
jaal_status ApiTabNavigate(jaal_handle tab, jaal_nav_action action) {
  ScopedApiCall call;
  return Engine::Get().TabNavigate(tab, action);
}

constexpr jaal_engine_api kApi = {
    .struct_size = sizeof(jaal_engine_api),
    .abi_major = JAAL_ENGINE_ABI_MAJOR,
    .abi_minor = JAAL_ENGINE_ABI_MINOR,
    .set_event_sink = &ApiSetEventSink,
    .quit = &ApiQuit,
    .window_create = &ApiWindowCreate,
    .window_close = &ApiWindowClose,
    .window_native_handle = &ApiWindowNativeHandle,
    .window_set_page_rect = &ApiWindowSetPageRect,
    .tab_open = &ApiTabOpen,
    .tab_close = &ApiTabClose,
    .tab_activate = &ApiTabActivate,
    .tab_load = &ApiTabLoad,
    .set_shortcut_filter = &ApiSetShortcutFilter,
    .window_set_page_placement = &ApiWindowSetPagePlacement,
    .window_attach_view = &ApiWindowAttachView,
    .window_update_view = &ApiWindowUpdateView,
    .window_detach_view = &ApiWindowDetachView,
    .window_focus_page = &ApiWindowFocusPage,
    .window_set_state = &ApiWindowSetState,
    .window_create_with = &ApiWindowCreateWith,
    .window_set_style = &ApiWindowSetStyle,
    .window_begin_drag = &ApiWindowBeginDrag,
    .tab_navigate = &ApiTabNavigate,
};

// The size of the first table version (1.0), which ends before the first
// call 1.1 added. A UI library built against a later minor version passes a
// bigger size and checks struct_size before using newer calls; one built
// against an older version must pass at least this.
constexpr uint32_t kFirstApiSize =
    offsetof(jaal_engine_api, set_shortcut_filter);

}  // namespace

// static
const jaal_engine_api* Engine::GetApi(uint32_t abi_major,
                                      uint32_t struct_size) {
  if (abi_major != JAAL_ENGINE_ABI_MAJOR || struct_size < kFirstApiSize) {
    LOG(ERROR) << "jaal: the UI library asked for ABI " << abi_major
               << " with a table of " << struct_size << " bytes; the engine is "
               << JAAL_ENGINE_ABI_MAJOR << " with " << kFirstApiSize;
    return nullptr;
  }
  return &kApi;
}

}  // namespace jaal
