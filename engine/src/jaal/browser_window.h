#ifndef JAAL_BROWSER_WINDOW_H_
#define JAAL_BROWSER_WINDOW_H_

#include <map>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "base/memory/raw_ptr.h"
#include "base/memory/weak_ptr.h"
#include "build/build_config.h"
#include "chrome/browser/ui/browser_window.h"
#include "chrome/browser/ui/exclusive_access/exclusive_access_context.h"
#include "chrome/browser/ui/unload_controller.h"
#include "jaal/public/jaal_engine.h"
#include "ui/base/accelerators/accelerator.h"
#include "ui/events/event_handler.h"
#include "ui/gfx/geometry/rect.h"
#include "ui/gfx/geometry/size.h"
#include "ui/views/widget/widget.h"
#include "ui/views/widget/widget_observer.h"

#if BUILDFLAG(IS_WIN)
#include "ui/base/win/hwnd_subclass.h"
#endif

class BrowserWindowInterface;

namespace views {
class WebView;
}

namespace jaal {

class JaalLocationBar;
class JaalModalDialogHost;
class PageAreaView;

// A browser window with no Chrome UI: a Views widget that holds the active
// tab's page and, above it, the app's own native views (the sidebar, the
// command bar), which the app draws. Each top-level window is a views::Widget
// so Chrome's dropdowns and dialogs have a parent to attach to.
//
// Most method bodies are copied from Chrome's own toolbar-less window,
// WebUIBrowserWindow (chrome/browser/ui/webui_browser/webui_browser_window.cc,
// Chromium 156.0.8078.88). That class hosts its tabs inside a WebUI page through
// GuestContents, which Chrome doesn't allow in release builds, so we can't
// subclass it; we show the page in a plain views::WebView instead.
class JaalBrowserWindow : public BrowserWindow,
                          public ui::AcceleratorProvider,
                          public ExclusiveAccessContext,
                          public views::WidgetObserver,
#if BUILDFLAG(IS_WIN)
                          public ui::HWNDMessageFilter,
#endif
                          public ui::EventHandler {
 public:
  explicit JaalBrowserWindow(BrowserWindowInterface* browser);
  JaalBrowserWindow(const JaalBrowserWindow&) = delete;
  JaalBrowserWindow& operator=(const JaalBrowserWindow&) = delete;
  ~JaalBrowserWindow() override;

  // Returns the jaal window for `browser`, or null when Chrome made its own.
  static JaalBrowserWindow* FromBrowser(BrowserWindowInterface* browser);

  BrowserWindowInterface* browser() { return browser_.get(); }
  views::Widget* widget() { return widget_.get(); }

  // Where the page goes in the client area. Null fills the window.
  void SetPageRect(std::optional<gfx::Rect> rect);
  void SetPagePlacement(const jaal_placement& placement);
  gfx::Rect GetContentsBoundsInScreen() const;
  // The page's rectangle in the window's client area.
  gfx::Rect GetContentsBoundsInWidget() const;

  // The app's native views inside this window (jaal_engine.h).
  jaal_status AttachView(void* native_view, const jaal_view_options& options);
  jaal_status UpdateView(void* native_view, const jaal_view_options& options);
  jaal_status DetachView(void* native_view);
  void FocusPage();
  void SetStyle(const jaal_window_style& style);
  void BeginDrag();

  jaal_window_state GetState() const;
  void SetState(jaal_window_state state);
  // The client area in DIPs, and physical pixels per DIP.
  gfx::Size GetClientSize() const;
  float GetScale() const;

  // BrowserWindow:
  gfx::NativeWindow GetNativeWindow() const override;
  bool IsOnCurrentWorkspace() const override;
  bool IsVisibleOnScreen() const override;
  void SetTopControlsShownRatio(content::WebContents* web_contents,
                                float ratio) override;
  bool DoBrowserControlsShrinkRendererSize(
      const content::WebContents* contents) const override;
  ui::NativeTheme* GetNativeTheme() override;
  const ui::ThemeProvider* GetThemeProvider() const override;
  const ui::ColorProvider* GetColorProvider() const override;
  int GetTopControlsHeight() const override;
  void SetTopControlsGestureScrollInProgress(bool in_progress) override;
  std::vector<StatusBubble*> GetStatusBubbles() override;
  void UpdateTitleBar() override;
  void UpdateLoadingAnimations(bool is_visible) override;
  void OnActiveTabChanged(content::WebContents* old_contents,
                          content::WebContents* new_contents,
                          int index,
                          int reason) override;
  void OnTabDetached(content::WebContents* contents, bool was_active) override;
  gfx::Size GetContentsSize() const override;
  void SetContentsSize(const gfx::Size& size) override;
  autofill::AutofillBubbleHandler* GetAutofillBubbleHandler() override;
  LocationBar* GetLocationBar() const override;
  ui::AcceleratorProvider* GetAcceleratorProvider() override;
  PinnedToolbarActions* GetPinnedToolbarActions() override;
  void SetFocusToLocationBar(bool is_user_initiated) override;
  void UpdateReloadStopState(bool is_loading, bool force) override;
  void UpdateToolbar(content::WebContents* contents) override;
  bool UpdateToolbarSecurityState() override;
  void UpdateCustomTabBarVisibility(bool visible, bool animate) override;
  void ResetToolbarTabState(content::WebContents* contents) override;
  void FocusToolbar() override;
  void ToolbarSizeChanged(bool is_animating) override;
  void TabDraggingStatusChanged(bool is_dragging) override;
  void LinkOpeningFromGesture(WindowOpenDisposition disposition) override;
  void FocusAppMenu() override;
  bool IsTabStripEditable() const override;
  void DisableTabStripEditingForTesting() override;
  bool IsToolbarVisible() const override;
  bool IsToolbarShowing() const override;
  bool IsLocationBarVisible() const override;
  void ShowUpdateChromeDialog() override;
  void ShowIntentPickerBubble(
      std::vector<apps::IntentPickerAppInfo> app_info,
      bool show_stay_in_chrome,
      bool show_remember_selection,
      apps::IntentPickerBubbleType bubble_type,
      const std::optional<url::Origin>& initiating_origin,
      IntentPickerResponse callback) override;
  void ShowBookmarkBubble(const GURL& url, bool already_bookmarked) override;
  ShowTranslateBubbleResult ShowTranslateBubble(
      content::WebContents* contents,
      translate::TranslateStep step,
      const std::string& source_language,
      const std::string& target_language,
      translate::TranslateErrors error_type,
      bool is_user_gesture) override;
  DownloadBubbleUIController* GetDownloadBubbleUIController() override;
  void ConfirmBrowserCloseWithPendingDownloads(
      int download_count,
      UnloadController::DownloadCloseType dialog_type,
      base::OnceCallback<void(bool)> callback) override;
  void ShowAppMenu() override;
  void PreHandleDragUpdate(const content::DropData& drop_data,
                           const gfx::PointF& point) override;
  void PreHandleDragExit() override;
  void HandleDragEnded() override;
  content::KeyboardEventProcessingResult PreHandleKeyboardEvent(
      const input::NativeWebKeyboardEvent& event) override;
  bool HandleKeyboardEvent(const input::NativeWebKeyboardEvent& event) override;
  std::unique_ptr<FindBar> CreateFindBar() override;
  web_modal::WebContentsModalDialogHost* GetWebContentsModalDialogHost()
      override;
  web_modal::WebContentsModalDialogHost* GetWebContentsModalDialogHostFor(
      content::WebContents* web_contents) override;
  void ShowAvatarBubbleFromAvatarButton(bool is_source_accelerator) override;
  void MaybeShowProfileSwitchIPH() override;
  void MaybeShowSupervisedUserProfileSignInIPH() override;
  void ShowHatsDialog(
      const std::string& site_id,
      const std::optional<std::string>& hats_histogram_name,
      const std::optional<uint64_t> hats_survey_ukm_id,
      base::OnceClosure success_callback,
      base::OnceClosure failure_callback,
      const SurveyBitsData& product_specific_bits_data,
      const SurveyStringData& product_specific_string_data) override;
  ExclusiveAccessContext* GetExclusiveAccessContext() override;
  std::string GetWorkspace() const override;
  bool IsVisibleOnAllWorkspaces() const override;
  void ShowEmojiPanel() override;
  std::unique_ptr<content::EyeDropper> OpenEyeDropper(
      content::RenderFrameHost* frame,
      content::EyeDropperListener* listener) override;
  void ShowCaretBrowsingDialog() override;
  void CreateTabSearchBubble() override;
  void CloseTabSearchBubble() override;
  void ShowIncognitoClearBrowsingDataDialog() override;
  void ShowIncognitoHistoryDisclaimerDialog() override;
  bool IsUnframedModeEnabled() const override;
  bool GetCanResize() override;
  ui::mojom::WindowShowState GetWindowShowState() const override;
  void ShowChromeLabs() override;
  BrowserView* AsBrowserView() override;

  // ui::BaseWindow:
  void Show() override;
  void ShowInactive() override;
  void Hide() override;
  bool IsVisible() const override;
  void SetBounds(const gfx::Rect& bounds) override;
  void Close() override;
  void Activate() override;
  void Deactivate() override;
  bool IsActive() const override;
  gfx::Rect GetBounds() const override;
  bool IsMaximized() const override;
  bool IsMinimized() const override;
  bool IsFullscreen() const override;  // Also in ExclusiveAccessContext.
  gfx::Rect GetRestoredBounds() const override;
  ui::mojom::WindowShowState GetRestoredState() const override;
  void Maximize() override;
  void Minimize() override;
  void Restore() override;
  void FlashFrame(bool flash) override;
  ui::ZOrderLevel GetZOrderLevel() const override;
  void SetZOrderLevel(ui::ZOrderLevel order) override;

  // ExclusiveAccessContext:
  Profile* GetProfile() override;
  void EnterFullscreen(const url::Origin& origin,
                       ExclusiveAccessBubbleType bubble_type,
                       FullscreenTabParams fullscreen_tab_params) override;
  void ExitFullscreen() override;
  void UpdateExclusiveAccessBubble(
      const ExclusiveAccessBubbleParams& params,
      ExclusiveAccessBubbleHideCallback first_hide_callback) override;
  bool IsExclusiveAccessBubbleDisplayed() const override;
  void OnExclusiveAccessUserInput() override;
  content::WebContents* GetWebContentsForExclusiveAccess() override;
  bool CanUserEnterFullscreen() const override;
  bool CanUserExitFullscreen() const override;

  // ui::AcceleratorProvider: jaal has no Chrome shortcuts yet (M2 asks the
  // app), so menus show none.
  bool GetAcceleratorForCommandId(int command_id,
                                  ui::Accelerator* accelerator) const override;

  // views::WidgetObserver:
  void OnWidgetBoundsChanged(views::Widget* widget,
                             const gfx::Rect& new_bounds) override;
  void OnWidgetActivationChanged(views::Widget* widget, bool active) override;
  void OnWidgetShowStateChanged(views::Widget* widget) override;

  // ui::EventHandler:
  void OnMouseEvent(ui::MouseEvent* event) override;

#if BUILDFLAG(IS_WIN)
  // ui::HWNDMessageFilter:
  bool FilterMessage(HWND hwnd,
                     UINT message,
                     WPARAM w_param,
                     LPARAM l_param,
                     LRESULT* l_result) override;
#endif

 protected:
  // BrowserWindow:
  void DeleteBrowserWindow() final;

 private:
  class WidgetDelegate;

  void PaintAsActiveChanged();
  void OnWindowCloseRequested(views::Widget::ClosedReason close_reason);
  void ProcessFullscreen(bool fullscreen);
  void RaiseAttachedViewsLater();

  const raw_ptr<BrowserWindowInterface> browser_;
  std::unique_ptr<WidgetDelegate> widget_delegate_;
  std::unique_ptr<views::Widget> widget_;
  raw_ptr<PageAreaView> page_area_ = nullptr;
  raw_ptr<views::WebView> web_view_ = nullptr;
  std::unique_ptr<JaalLocationBar> location_bar_;
  std::unique_ptr<JaalModalDialogHost> modal_dialog_host_;
  base::CallbackListSubscription paint_as_active_subscription_;
  // True while one of the app's views has keyboard focus (Windows).
  bool app_view_has_focus_ = false;
  // No system title bar: see jaal_window_options.
  bool frameless_ = false;
  // True while a wheel message is being handed to one of the app's views.
  bool forwarding_wheel_ = false;

  base::WeakPtrFactory<JaalBrowserWindow> weak_factory_{this};
};

}  // namespace jaal

#endif  // JAAL_BROWSER_WINDOW_H_
