#include "jaal/browser_window.h"

#include <utility>

#include "base/check.h"
#include "base/notimplemented.h"
#include "chrome/browser/profiles/profile.h"
#include "chrome/browser/themes/theme_service.h"
#include "chrome/browser/ui/browser_active_state_manager/browser_active_state_manager.h"
#include "chrome/browser/ui/browser_init_state.h"
#include "chrome/browser/ui/browser_window/public/browser_window_features.h"
#include "chrome/browser/ui/browser_window/public/browser_window_interface.h"
#include "chrome/browser/ui/browser_window_state.h"
#include "chrome/browser/ui/exclusive_access/exclusive_access_manager.h"
#include "chrome/browser/ui/exclusive_access/fullscreen_controller.h"
#include "chrome/browser/ui/find_bar/find_bar.h"
#include "chrome/browser/ui/fullscreen/browser_window_fullscreen_controller.h"
#include "chrome/browser/ui/tabs/tab_strip_model.h"
#include "chrome/browser/ui/webui_browser/browser_elements_webui_browser.h"
#include "chrome/browser/ui/window_metadata/window_metadata_controller.h"
#include "components/input/native_web_keyboard_event.h"
#include "content/public/browser/keyboard_event_processing_result.h"
#include "content/public/browser/web_contents.h"
#include "jaal/engine.h"
#include "jaal/location_bar.h"
#include "jaal/modal_dialog_host.h"
#include "ui/base/mojom/window_show_state.mojom.h"
#include "ui/display/types/display_constants.h"
#include "ui/native_theme/native_theme.h"
#include "ui/views/controls/webview/webview.h"
#include "ui/views/view.h"
#include "ui/views/widget/widget_delegate.h"

namespace jaal {

namespace {

const char kJaalBrowserWindowKey[] = "__JAAL_BROWSER_WINDOW__";

}  // namespace

// The widget's contents: the page, at the rect the app gives or filling the
// window. M2 puts GPUI's view beside it.
class PageAreaView : public views::View {
 public:
  explicit PageAreaView(Profile* profile) {
    web_view_ = AddChildView(std::make_unique<views::WebView>(profile));
  }

  views::WebView* web_view() { return web_view_; }

  void SetPageRect(std::optional<gfx::Rect> rect) {
    page_rect_ = rect;
    InvalidateLayout();
  }

  // views::View:
  void Layout(PassKey) override {
    web_view_->SetBoundsRect(page_rect_.value_or(GetLocalBounds()));
  }

 private:
  raw_ptr<views::WebView> web_view_ = nullptr;
  std::optional<gfx::Rect> page_rect_;
};

// Copied from WebUIBrowserWindow::WidgetDelegate, without the WebUI client
// view.
class JaalBrowserWindow::WidgetDelegate : public views::WidgetDelegate {
 public:
  explicit WidgetDelegate(JaalBrowserWindow* window) : window_(window) {
    const auto& params =
        BrowserInitState::From(window_->browser())->create_params();
    SetCanResize(params.can_resize);
    SetCanMaximize(params.can_maximize);
    SetCanFullscreen(params.can_fullscreen);
    SetCanMinimize(true);
  }

  // views::WidgetDelegate:
  bool ShouldSaveWindowPlacement() const override {
    // Fullscreen bounds aren't the window's real placement.
    return !window_->IsFullscreen() &&
           chrome::ShouldSaveWindowPlacement(window_->browser());
  }

  void SaveWindowPlacement(const gfx::Rect& bounds,
                           ui::mojom::WindowShowState show_state) override {
    CHECK(ShouldSaveWindowPlacement());
    views::WidgetDelegate::SaveWindowPlacement(bounds, show_state);
    chrome::SaveWindowPlacement(window_->browser(), bounds, show_state);
  }

  bool GetSavedWindowPlacement(
      const views::Widget* widget,
      gfx::Rect* bounds,
      ui::mojom::WindowShowState* show_state) const override {
    chrome::GetSavedWindowBoundsAndShowState(window_->browser(), bounds,
                                             show_state);
    return true;
  }

  std::u16string GetWindowTitle() const override {
    return WindowMetadataController::From(window_->browser())
        ->GetWindowTitleForCurrentTab(/*include_app_name=*/true);
  }

 private:
  raw_ptr<JaalBrowserWindow> window_;
};

JaalBrowserWindow::JaalBrowserWindow(BrowserWindowInterface* browser)
    : browser_(browser) {
  location_bar_ = std::make_unique<JaalLocationBar>(browser_);
  widget_delegate_ = std::make_unique<WidgetDelegate>(this);
  widget_ = std::make_unique<views::Widget>();
  widget_->AddObserver(this);

  views::Widget::InitParams params(
      views::Widget::InitParams::CLIENT_OWNS_WIDGET);
  params.name = "JaalBrowserWindow";
  chrome::GetSavedWindowBoundsAndShowState(browser_, &params.bounds,
                                           &params.show_state);
  params.delegate = widget_delegate_.get();
  widget_->Init(std::move(params));
  widget_->SetNativeWindowProperty(kJaalBrowserWindowKey, this);
  // Lets beforeunload handlers cancel a close before the window hides.
  widget_->MakeCloseSynchronous(base::BindOnce(
      &JaalBrowserWindow::OnWindowCloseRequested, base::Unretained(this)));

  // IsWebUIBrowserEnabled() answers true in jaal, so Chrome gave this browser
  // Webium's element tracker, which needs the widget.
  if (auto* elements = BrowserElementsWebUiBrowser::From(browser_)) {
    elements->Init(widget_.get());
  }

  // A framed widget takes its contents through the client view.
  page_area_ = widget_->SetClientContentsView(
      std::make_unique<PageAreaView>(browser_->GetProfile()));
  web_view_ = page_area_->web_view();

  modal_dialog_host_ = std::make_unique<JaalModalDialogHost>(this);

  paint_as_active_subscription_ =
      widget_->RegisterPaintAsActiveChangedCallback(base::BindRepeating(
          &JaalBrowserWindow::PaintAsActiveChanged, base::Unretained(this)));

  Engine::Get().OnWindowCreated(this);
}

JaalBrowserWindow::~JaalBrowserWindow() {
  Engine::Get().OnWindowDestroyed(this);
  browser_->GetFeatures().TearDownPreBrowserWindowDestruction();
  modal_dialog_host_.reset();
  web_view_ = nullptr;
  page_area_ = nullptr;
  widget_->RemoveObserver(this);
  widget_.reset();
}

// static
JaalBrowserWindow* JaalBrowserWindow::FromBrowser(
    BrowserWindowInterface* browser) {
  if (!browser->GetWindow() || !browser->GetWindow()->GetNativeWindow()) {
    return nullptr;
  }
  views::Widget* widget = views::Widget::GetWidgetForNativeWindow(
      browser->GetWindow()->GetNativeWindow());
  return widget ? static_cast<JaalBrowserWindow*>(
                      widget->GetNativeWindowProperty(kJaalBrowserWindowKey))
                : nullptr;
}

void JaalBrowserWindow::SetPageRect(std::optional<gfx::Rect> rect) {
  page_area_->SetPageRect(rect);
  modal_dialog_host_->NotifyPositionRequiresUpdate();
}

gfx::Rect JaalBrowserWindow::GetContentsBoundsInScreen() const {
  return web_view_->GetBoundsInScreen();
}

void JaalBrowserWindow::DeleteBrowserWindow() {
  delete this;
}

// Copied from WebUIBrowserWindow::Show(), which copies BrowserView::Show().
void JaalBrowserWindow::Show() {
#if !BUILDFLAG(IS_WIN) && !BUILDFLAG(IS_CHROMEOS)
  // The browser must be the active one as soon as Show() is called; other
  // platforms only report activation on the next loop turn.
  BrowserActiveStateManager::From(browser_)->DidBecomeActive();
#endif
  if (widget_->IsVisible()) {
    widget_->Activate();
    return;
  }
  widget_->Show();
  if (content::WebContents* contents = web_view_->GetWebContents()) {
    contents->SetInitialFocus();
  }
}

void JaalBrowserWindow::PaintAsActiveChanged() {
  if (widget_->ShouldPaintAsActive()) {
    BrowserActiveStateManager::From(browser_)->DidBecomeActive();
  } else {
    BrowserActiveStateManager::From(browser_)->DidBecomeInactive();
  }
}

void JaalBrowserWindow::ShowInactive() {
  if (!widget_->IsVisible()) {
    widget_->ShowInactive();
  }
}

void JaalBrowserWindow::Hide() {
  widget_->Hide();
}

bool JaalBrowserWindow::IsVisible() const {
  return widget_->IsVisible();
}

void JaalBrowserWindow::SetBounds(const gfx::Rect& bounds) {
  widget_->SetBounds(bounds);
}

void JaalBrowserWindow::Close() {
  // Doesn't close at once: the OS asks back through OnWindowCloseRequested(),
  // where beforeunload handlers may stop it. Once they pass, Chrome calls
  // Close() again and the window really closes.
  widget_->Close();
}

// Copied from WebUIBrowserWindow::OnWindowCloseRequested().
void JaalBrowserWindow::OnWindowCloseRequested(
    views::Widget::ClosedReason close_reason) {
  if (!UnloadController::From(browser_)->HandleBeforeClose()) {
    // The synchronous close callback is used up by each Close(), and Close()
    // usually runs twice on the way out.
    widget_->MakeCloseSynchronous(base::BindOnce(
        &JaalBrowserWindow::OnWindowCloseRequested, base::Unretained(this)));
    return;
  }
  UnloadController::From(browser_)->OnWindowClosing();
  if (!browser_->GetTabStripModel()->empty()) {
    // Hide now so the window looks closed, and close the tabs so their
    // renderers can shut down. We're called again once the strip is empty.
    widget_->Hide();
  }
}

void JaalBrowserWindow::Activate() {
  widget_->Activate();
}

void JaalBrowserWindow::Deactivate() {
  widget_->Deactivate();
}

bool JaalBrowserWindow::IsActive() const {
  return widget_->IsActive();
}

void JaalBrowserWindow::FlashFrame(bool flash) {
  widget_->FlashFrame(flash);
}

ui::ZOrderLevel JaalBrowserWindow::GetZOrderLevel() const {
  return widget_->GetZOrderLevel();
}

void JaalBrowserWindow::SetZOrderLevel(ui::ZOrderLevel order) {
  widget_->SetZOrderLevel(order);
}

gfx::NativeWindow JaalBrowserWindow::GetNativeWindow() const {
  return widget_->GetNativeWindow();
}

bool JaalBrowserWindow::IsOnCurrentWorkspace() const {
  return true;
}

bool JaalBrowserWindow::IsVisibleOnScreen() const {
  return widget_->IsVisibleOnScreen();
}

void JaalBrowserWindow::SetTopControlsShownRatio(
    content::WebContents* web_contents,
    float ratio) {}

bool JaalBrowserWindow::DoBrowserControlsShrinkRendererSize(
    const content::WebContents* contents) const {
  return false;
}

ui::NativeTheme* JaalBrowserWindow::GetNativeTheme() {
  return ui::NativeTheme::GetInstanceForNativeUi();
}

const ui::ThemeProvider* JaalBrowserWindow::GetThemeProvider() const {
  return &ThemeService::GetThemeProviderForProfile(browser_->GetProfile());
}

const ui::ColorProvider* JaalBrowserWindow::GetColorProvider() const {
  return widget_->GetColorProvider();
}

int JaalBrowserWindow::GetTopControlsHeight() const {
  return 0;
}

void JaalBrowserWindow::SetTopControlsGestureScrollInProgress(
    bool in_progress) {}

std::vector<StatusBubble*> JaalBrowserWindow::GetStatusBubbles() {
  return {};
}

void JaalBrowserWindow::UpdateTitleBar() {
  widget_->UpdateWindowTitle();
}

void JaalBrowserWindow::UpdateLoadingAnimations(bool is_visible) {}

void JaalBrowserWindow::OnActiveTabChanged(content::WebContents* old_contents,
                                           content::WebContents* new_contents,
                                           int index,
                                           int reason) {
  // One web view per window; a tab switch swaps its contents, which is
  // Chrome's own path (BrowserView does the same) and hides the old page.
  web_view_->SetWebContents(new_contents);
  if (widget_->IsActive()) {
    new_contents->Focus();
  }
}

void JaalBrowserWindow::OnTabDetached(content::WebContents* contents,
                                      bool was_active) {
  // Let go of the page before Chrome destroys it or moves it to another
  // window.
  if (was_active && web_view_->GetWebContents() == contents) {
    web_view_->SetWebContents(nullptr);
  }
}

gfx::Size JaalBrowserWindow::GetContentsSize() const {
  return web_view_->size();
}

void JaalBrowserWindow::SetContentsSize(const gfx::Size& size) {
  gfx::Rect bounds = GetBounds();
  bounds.set_size(bounds.size() + size - GetContentsSize());
  SetBounds(bounds);
}

autofill::AutofillBubbleHandler* JaalBrowserWindow::GetAutofillBubbleHandler() {
  NOTIMPLEMENTED_LOG_ONCE();
  return nullptr;
}

LocationBar* JaalBrowserWindow::GetLocationBar() const {
  return location_bar_.get();
}

ui::AcceleratorProvider* JaalBrowserWindow::GetAcceleratorProvider() {
  return this;
}

bool JaalBrowserWindow::GetAcceleratorForCommandId(
    int command_id,
    ui::Accelerator* accelerator) const {
  return false;
}

PinnedToolbarActions* JaalBrowserWindow::GetPinnedToolbarActions() {
  return nullptr;
}

void JaalBrowserWindow::SetFocusToLocationBar(bool is_user_initiated) {
  NOTIMPLEMENTED_LOG_ONCE();
}

void JaalBrowserWindow::UpdateReloadStopState(bool is_loading, bool force) {}

void JaalBrowserWindow::UpdateToolbar(content::WebContents* contents) {}

bool JaalBrowserWindow::UpdateToolbarSecurityState() {
  return false;
}

void JaalBrowserWindow::UpdateCustomTabBarVisibility(bool visible,
                                                     bool animate) {}

void JaalBrowserWindow::ResetToolbarTabState(content::WebContents* contents) {}

void JaalBrowserWindow::FocusToolbar() {}

void JaalBrowserWindow::ToolbarSizeChanged(bool is_animating) {}

void JaalBrowserWindow::TabDraggingStatusChanged(bool is_dragging) {}

void JaalBrowserWindow::LinkOpeningFromGesture(
    WindowOpenDisposition disposition) {}

void JaalBrowserWindow::FocusAppMenu() {}

bool JaalBrowserWindow::IsTabStripEditable() const {
  return true;
}

void JaalBrowserWindow::DisableTabStripEditingForTesting() {}

bool JaalBrowserWindow::IsToolbarVisible() const {
  return false;
}

bool JaalBrowserWindow::IsToolbarShowing() const {
  return false;
}

bool JaalBrowserWindow::IsLocationBarVisible() const {
  return false;
}

void JaalBrowserWindow::ShowUpdateChromeDialog() {}

void JaalBrowserWindow::ShowIntentPickerBubble(
    std::vector<apps::IntentPickerAppInfo> app_info,
    bool show_stay_in_chrome,
    bool show_remember_selection,
    apps::IntentPickerBubbleType bubble_type,
    const std::optional<url::Origin>& initiating_origin,
    IntentPickerResponse callback) {
  NOTIMPLEMENTED_LOG_ONCE();
}

void JaalBrowserWindow::ShowBookmarkBubble(const GURL& url,
                                           bool already_bookmarked) {
  NOTIMPLEMENTED_LOG_ONCE();
}

ShowTranslateBubbleResult JaalBrowserWindow::ShowTranslateBubble(
    content::WebContents* contents,
    translate::TranslateStep step,
    const std::string& source_language,
    const std::string& target_language,
    translate::TranslateErrors error_type,
    bool is_user_gesture) {
  return ShowTranslateBubbleResult::kBrowserWindowNotValid;
}

DownloadBubbleUIController* JaalBrowserWindow::GetDownloadBubbleUIController() {
  return nullptr;
}

void JaalBrowserWindow::ConfirmBrowserCloseWithPendingDownloads(
    int download_count,
    UnloadController::DownloadCloseType dialog_type,
    base::OnceCallback<void(bool)> callback) {
  // TODO(jaal): ask the app (docs/engine.md › Quitting). Until then a close
  // proceeds, which beats Webium's stub: it never answers and hangs the quit.
  std::move(callback).Run(true);
}

void JaalBrowserWindow::ShowAppMenu() {}

void JaalBrowserWindow::PreHandleDragUpdate(const content::DropData& drop_data,
                                            const gfx::PointF& point) {}

void JaalBrowserWindow::PreHandleDragExit() {}

void JaalBrowserWindow::HandleDragEnded() {}

content::KeyboardEventProcessingResult
JaalBrowserWindow::PreHandleKeyboardEvent(
    const input::NativeWebKeyboardEvent& event) {
  // M2 asks the app about reserved shortcuts here. Until then every key goes
  // to the page.
  return content::KeyboardEventProcessingResult::NOT_HANDLED;
}

bool JaalBrowserWindow::HandleKeyboardEvent(
    const input::NativeWebKeyboardEvent& event) {
  return false;
}

std::unique_ptr<FindBar> JaalBrowserWindow::CreateFindBar() {
  // The find bar is the app's (M6). No shortcut reaches find yet.
  NOTIMPLEMENTED_LOG_ONCE();
  return nullptr;
}

web_modal::WebContentsModalDialogHost*
JaalBrowserWindow::GetWebContentsModalDialogHost() {
  return modal_dialog_host_.get();
}

web_modal::WebContentsModalDialogHost*
JaalBrowserWindow::GetWebContentsModalDialogHostFor(
    content::WebContents* web_contents) {
  return modal_dialog_host_.get();
}

void JaalBrowserWindow::ShowAvatarBubbleFromAvatarButton(
    bool is_source_accelerator) {}

void JaalBrowserWindow::MaybeShowProfileSwitchIPH() {}

void JaalBrowserWindow::MaybeShowSupervisedUserProfileSignInIPH() {}

void JaalBrowserWindow::ShowHatsDialog(
    const std::string& site_id,
    const std::optional<std::string>& hats_histogram_name,
    const std::optional<uint64_t> hats_survey_ukm_id,
    base::OnceClosure success_callback,
    base::OnceClosure failure_callback,
    const SurveyBitsData& product_specific_bits_data,
    const SurveyStringData& product_specific_string_data) {
  std::move(failure_callback).Run();
}

ExclusiveAccessContext* JaalBrowserWindow::GetExclusiveAccessContext() {
  return this;
}

std::string JaalBrowserWindow::GetWorkspace() const {
  return std::string();
}

bool JaalBrowserWindow::IsVisibleOnAllWorkspaces() const {
  return false;
}

void JaalBrowserWindow::ShowEmojiPanel() {
  widget_->ShowEmojiPanel();
}

std::unique_ptr<content::EyeDropper> JaalBrowserWindow::OpenEyeDropper(
    content::RenderFrameHost* frame,
    content::EyeDropperListener* listener) {
  NOTIMPLEMENTED_LOG_ONCE();
  return nullptr;
}

void JaalBrowserWindow::ShowCaretBrowsingDialog() {}

void JaalBrowserWindow::CreateTabSearchBubble() {}

void JaalBrowserWindow::CloseTabSearchBubble() {}

void JaalBrowserWindow::ShowIncognitoClearBrowsingDataDialog() {}

void JaalBrowserWindow::ShowIncognitoHistoryDisclaimerDialog() {}

bool JaalBrowserWindow::IsUnframedModeEnabled() const {
  return false;
}

bool JaalBrowserWindow::GetCanResize() {
  return widget_delegate_->CanResize();
}

ui::mojom::WindowShowState JaalBrowserWindow::GetWindowShowState() const {
  if (IsMaximized()) {
    return ui::mojom::WindowShowState::kMaximized;
  }
  if (IsMinimized()) {
    return ui::mojom::WindowShowState::kMinimized;
  }
  if (IsFullscreen()) {
    return ui::mojom::WindowShowState::kFullscreen;
  }
  return ui::mojom::WindowShowState::kDefault;
}

void JaalBrowserWindow::ShowChromeLabs() {}

BrowserView* JaalBrowserWindow::AsBrowserView() {
  return nullptr;
}

gfx::Rect JaalBrowserWindow::GetBounds() const {
  return widget_->GetWindowBoundsInScreen();
}

bool JaalBrowserWindow::IsMaximized() const {
  return widget_->IsMaximized();
}

bool JaalBrowserWindow::IsMinimized() const {
  return widget_->IsMinimized();
}

bool JaalBrowserWindow::IsFullscreen() const {
  return widget_->IsFullscreen();
}

gfx::Rect JaalBrowserWindow::GetRestoredBounds() const {
  return widget_->GetRestoredBounds();
}

ui::mojom::WindowShowState JaalBrowserWindow::GetRestoredState() const {
  return IsMaximized() ? ui::mojom::WindowShowState::kMaximized
                       : ui::mojom::WindowShowState::kDefault;
}

void JaalBrowserWindow::Maximize() {
  widget_->Maximize();
}

void JaalBrowserWindow::Minimize() {
  widget_->Minimize();
}

void JaalBrowserWindow::Restore() {
  widget_->Restore();
}

Profile* JaalBrowserWindow::GetProfile() {
  return browser_->GetProfile();
}

// Fullscreen follows WebUIBrowserExclusiveAccessContext, without its exit
// notice bubble; the app shows that notice (M6).
void JaalBrowserWindow::EnterFullscreen(
    const url::Origin& origin,
    ExclusiveAccessBubbleType bubble_type,
    FullscreenTabParams fullscreen_tab_params) {
  if (IsFullscreen() &&
      fullscreen_tab_params.display_id == display::kInvalidDisplayId) {
    return;
  }
  ProcessFullscreen(true);
}

void JaalBrowserWindow::ExitFullscreen() {
  if (BrowserWindowFullscreenController::From(browser_)->IsForceFullscreen()) {
    return;
  }
  if (!IsFullscreen()) {
    return;
  }
  ProcessFullscreen(false);
}

void JaalBrowserWindow::ProcessFullscreen(bool fullscreen) {
  widget_->SetFullscreen(fullscreen);
  if (auto* manager = ExclusiveAccessManager::From(browser_)) {
    manager->fullscreen_controller()->WindowFullscreenStateChanged();
  }
}

void JaalBrowserWindow::UpdateExclusiveAccessBubble(
    const ExclusiveAccessBubbleParams& params,
    ExclusiveAccessBubbleHideCallback first_hide_callback) {
  if (first_hide_callback) {
    std::move(first_hide_callback)
        .Run(ExclusiveAccessBubbleHideReason::kNotShown);
  }
}

bool JaalBrowserWindow::IsExclusiveAccessBubbleDisplayed() const {
  return false;
}

void JaalBrowserWindow::OnExclusiveAccessUserInput() {}

content::WebContents* JaalBrowserWindow::GetWebContentsForExclusiveAccess() {
  return browser_->GetTabStripModel()->GetActiveWebContents();
}

bool JaalBrowserWindow::CanUserEnterFullscreen() const {
  return true;
}

bool JaalBrowserWindow::CanUserExitFullscreen() const {
  return true;
}

void JaalBrowserWindow::OnWidgetBoundsChanged(views::Widget* widget,
                                              const gfx::Rect& new_bounds) {
  // Widget::Init() already reports a size, before the dialog host exists.
  if (modal_dialog_host_) {
    modal_dialog_host_->NotifyPositionRequiresUpdate();
  }
}

}  // namespace jaal
