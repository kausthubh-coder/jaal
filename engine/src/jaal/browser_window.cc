#include "jaal/browser_window.h"

#include <algorithm>
#include <utility>

#include "base/auto_reset.h"
#include "base/check.h"
#include "base/containers/adapters.h"
#include "base/functional/bind.h"
#include "base/memory/weak_ptr.h"
#include "base/notimplemented.h"
#include "base/task/sequenced_task_runner.h"
#include "build/build_config.h"
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
#include "third_party/blink/public/common/input/web_input_event.h"
#include "third_party/skia/include/core/SkColor.h"
#include "ui/aura/window.h"
#include "ui/base/ime/input_method.h"
#include "ui/base/mojom/window_show_state.mojom.h"
#include "ui/compositor/compositor.h"
#include "ui/compositor/layer.h"
#include "ui/display/types/display_constants.h"
#include "ui/events/event.h"
#include "ui/events/types/event_type.h"
#include "ui/gfx/geometry/rect_conversions.h"
#include "ui/gfx/geometry/rounded_corners_f.h"
#include "cc/paint/paint_flags.h"
#include "ui/gfx/canvas.h"
#include "ui/gfx/geometry/point.h"
#include "ui/gfx/geometry/rect_f.h"
#include "ui/gfx/geometry/vector2d.h"
#include "ui/gfx/native_ui_types.h"
#include "ui/gfx/shadow_value.h"
#include "ui/gfx/skia_paint_util.h"
#include "ui/views/controls/native/native_view_host.h"
#include "ui/native_theme/native_theme.h"
#include "ui/views/background.h"
#include "ui/views/controls/webview/webview.h"
#include "ui/views/view.h"
#include "ui/views/widget/widget_delegate.h"

#if BUILDFLAG(IS_WIN)
#include <windows.h>

#include "ui/views/win/hwnd_util.h"
#endif

namespace jaal {

namespace {

const char kJaalBrowserWindowKey[] = "__JAAL_BROWSER_WINDOW__";

// One axis of a jaal_placement: `start` and `end` are distances from the two
// edges, `size` a length, any of them JAAL_AUTO. See jaal_engine.h.
void ResolveAxis(int start, int end, int size, int extent, int* out_start,
                 int* out_size) {
  const bool has_start = start != JAAL_AUTO;
  const bool has_end = end != JAAL_AUTO;
  const bool has_size = size != JAAL_AUTO;
  if (has_start && has_end) {
    const int room = extent - start - end;
    *out_size = has_size ? size : room;
    *out_start = has_size ? start + (room - size) / 2 : start;
  } else if (has_start) {
    *out_size = has_size ? size : extent - start;
    *out_start = start;
  } else if (has_end) {
    *out_size = has_size ? size : extent - end;
    *out_start = extent - end - *out_size;
  } else {
    *out_size = has_size ? size : extent;
    *out_start = (extent - *out_size) / 2;
  }
  *out_size = std::max(0, *out_size);
}

gfx::Rect ResolvePlacement(const jaal_placement& placement,
                           const gfx::Rect& client) {
  int x, y, width, height;
  ResolveAxis(placement.left, placement.right, placement.width, client.width(),
              &x, &width);
  ResolveAxis(placement.top, placement.bottom, placement.height,
              client.height(), &y, &height);
  return gfx::Rect(client.x() + x, client.y() + y, width, height);
}

}  // namespace

// The widget's contents: the page, the app's own native views above it (the
// sidebar, the command bar), and a blur layer under each see-through one.
//
// The app's views are child HWNDs. GPUI draws through DirectComposition, and
// a child HWND's DirectComposition surface shows above Chrome's own
// compositor window, with per-pixel alpha (bakeoff/results, M2 layout test).
class PageAreaView : public views::View {
 public:
  explicit PageAreaView(Profile* profile) {
    web_view_ = AddChildView(std::make_unique<views::WebView>(profile));
  }

  views::WebView* web_view() { return web_view_; }

  void SetPageRect(std::optional<gfx::Rect> rect) {
    page_rect_ = rect;
    page_placement_.reset();
    InvalidateLayout();
  }

  void SetPagePlacement(const jaal_placement& placement) {
    page_placement_ = placement;
    InvalidateLayout();
  }

  void SetStyle(const jaal_window_style& style) {
    style_ = style;
    web_view_->holder()->SetNativeViewCornerRadii(
        gfx::RoundedCornersF(style.page_corner_radius));
    SchedulePaint();
  }

  bool HasAttached(gfx::AcceleratedWidget native_view) const {
    return FindAttached(native_view) != attached_.end();
  }

  void Attach(gfx::AcceleratedWidget native_view,
              const jaal_view_options& options) {
    Attached view;
    view.native_view = native_view;
    view.needs_parent = true;
    attached_.push_back(std::move(view));
    Update(native_view, options);
  }

  void Update(gfx::AcceleratedWidget native_view,
              const jaal_view_options& options) {
    auto it = FindAttached(native_view);
    CHECK(it != attached_.end());
    it->options = options;
    if (options.backdrop_blur > 0 && !it->blur) {
      // Added after the web view, so its layer is above the page's.
      auto blur = std::make_unique<views::View>();
      blur->SetPaintToLayer();
      blur->layer()->SetName("JaalBackdropBlur");
      blur->layer()->SetFillsBoundsOpaquely(false);
      // A layer that paints nothing can be skipped along with its backdrop
      // filter, so it paints one step above transparent.
      blur->SetBackground(
          views::CreateSolidBackground(SkColorSetARGB(1, 255, 255, 255)));
      blur->SetCanProcessEventsWithinSubtree(false);
      it->blur = AddChildView(std::move(blur));
    } else if (options.backdrop_blur <= 0 && it->blur) {
      RemoveChildViewT(it->blur.ExtractAsDangling());
    }
    if (it->blur) {
      it->blur->layer()->SetBackgroundBlur(options.backdrop_blur);
      // Blurring at half resolution is what Chrome's own blurred surfaces
      // do; a 28 DIP blur hides the difference.
      it->blur->layer()->SetBackdropFilterQuality(0.5f);
      it->blur->layer()->SetRoundedCornerRadius(
          gfx::RoundedCornersF(options.backdrop_corner_radius));
      it->blur->layer()->SetIsFastRoundedCorner(true);
    }
    InvalidateLayout();
    PlaceAttachedLater();
  }

  void Detach(gfx::AcceleratedWidget native_view) {
    auto it = FindAttached(native_view);
    if (it == attached_.end()) {
      return;
    }
    if (it->blur) {
      RemoveChildViewT(it->blur.ExtractAsDangling());
    }
    attached_.erase(it);
#if BUILDFLAG(IS_WIN)
    // Hidden from a task of its own: hiding sends the view's window messages,
    // which must not reach the app while it is inside this call.
    base::SequencedTaskRunner::GetCurrentDefault()->PostTask(
        FROM_HERE, base::BindOnce(
                       [](HWND hwnd) {
                         if (::IsWindow(hwnd)) {
                           ::ShowWindow(hwnd, SW_HIDE);
                         }
                       },
                       native_view));
#endif
  }

  // Puts the app's views back above every other child window. Chrome adds
  // child windows of its own over the page (one per page for accessibility
  // and touch input), and a new one starts on top.
  void RaiseAttached() {
#if BUILDFLAG(IS_WIN)
    for (const Attached& view : attached_) {
      if (!view.needs_parent && ::IsWindow(view.native_view)) {
        ::SetWindowPos(view.native_view, HWND_TOP, 0, 0, 0, 0,
                       SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
      }
    }
#endif
  }

  // The topmost of the app's views that is shown at a screen point.
  gfx::AcceleratedWidget AttachedAt(const gfx::Point& screen_point) const {
#if BUILDFLAG(IS_WIN)
    for (const Attached& view : base::Reversed(attached_)) {
      RECT rect = {};
      if (::IsWindowVisible(view.native_view) &&
          ::GetWindowRect(view.native_view, &rect) &&
          screen_point.x() >= rect.left && screen_point.x() < rect.right &&
          screen_point.y() >= rect.top && screen_point.y() < rect.bottom) {
        return view.native_view;
      }
    }
#endif
    return gfx::kNullAcceleratedWidget;
  }

  bool IsAttachedOrChild(gfx::AcceleratedWidget window) const {
#if BUILDFLAG(IS_WIN)
    for (const Attached& view : attached_) {
      if (view.native_view == window || ::IsChild(view.native_view, window)) {
        return true;
      }
    }
#endif
    return false;
  }

  // views::View:
  void OnPaint(gfx::Canvas* canvas) override {
    if (!style_) {
      return;
    }
    cc::PaintFlags background;
    background.setShader(gfx::CreateGradientShader(
        gfx::Point(0, 0), gfx::Point(0, height()), style_->background_top,
        style_->background_bottom));
    canvas->DrawRect(GetLocalBounds(), background);

    // Under the page: its shadow, and the fill that shows outside the page's
    // rounded corners. The page itself covers the rest.
    const gfx::RectF page(web_view_->bounds());
    if (page.IsEmpty()) {
      return;
    }
    if (SkColorGetA(style_->page_shadow_color)) {
      cc::PaintFlags shadow;
      shadow.setAntiAlias(true);
      shadow.setColor(style_->background_bottom);
      shadow.setLooper(gfx::CreateShadowDrawLooper({gfx::ShadowValue(
          gfx::Vector2d(0, static_cast<int>(style_->page_shadow_offset_y)),
          style_->page_shadow_blur, style_->page_shadow_color)}));
      canvas->DrawRoundRect(page, style_->page_corner_radius, shadow);
    }
    if (SkColorGetA(style_->page_outline_color)) {
      cc::PaintFlags outline;
      outline.setAntiAlias(true);
      outline.setStyle(cc::PaintFlags::kStroke_Style);
      outline.setStrokeWidth(1.0f);
      outline.setColor(style_->page_outline_color);
      gfx::RectF ring = page;
      ring.Outset(0.5f);
      canvas->DrawRoundRect(ring, style_->page_corner_radius + 0.5f, outline);
    }
  }

  void Layout(PassKey) override {
    const gfx::Rect client = GetLocalBounds();
    web_view_->SetBoundsRect(page_placement_
                                 ? ResolvePlacement(*page_placement_, client)
                                 : page_rect_.value_or(client));
    // The page's shadow is painted here, around wherever the page is.
    SchedulePaint();
    for (const Attached& view : attached_) {
      if (view.blur) {
        gfx::Rect bounds = ResolvePlacement(view.options.placement, client);
        bounds.Inset(view.options.backdrop_inset);
        view.blur->SetBoundsRect(bounds);
        view.blur->SetVisible(view.options.visible);
      }
    }
    // Moving a child HWND sends it WM_SIZE at once. While the app is inside
    // one of its own engine calls that would re-enter it, so it waits for
    // the next task; a resize by the user places the views right here, in
    // step with the page.
    if (Engine::Get().in_api_call()) {
      PlaceAttachedLater();
    } else {
      PlaceAttached();
    }
  }

 private:
  struct Attached {
    gfx::AcceleratedWidget native_view = gfx::kNullAcceleratedWidget;
    jaal_view_options options = {};
    raw_ptr<views::View> blur = nullptr;
    // True until the view has been made a child of the window.
    bool needs_parent = false;
  };

  std::vector<Attached>::iterator FindAttached(
      gfx::AcceleratedWidget native_view) {
    return std::ranges::find(attached_, native_view, &Attached::native_view);
  }
  std::vector<Attached>::const_iterator FindAttached(
      gfx::AcceleratedWidget native_view) const {
    return std::ranges::find(attached_, native_view, &Attached::native_view);
  }

  void PlaceAttachedLater() {
    if (place_pending_) {
      return;
    }
    place_pending_ = true;
    base::SequencedTaskRunner::GetCurrentDefault()->PostTask(
        FROM_HERE, base::BindOnce(&PageAreaView::PlaceAttachedFromTask,
                                  weak_factory_.GetWeakPtr()));
  }

  void PlaceAttachedFromTask() {
    place_pending_ = false;
    PlaceAttached();
  }

  void PlaceAttached() {
#if BUILDFLAG(IS_WIN)
    if (!GetWidget()) {
      return;
    }
    const HWND parent = views::HWNDForWidget(GetWidget());
    const float scale = GetWidget()->GetCompositor()->device_scale_factor();
    // The app destroys its own views; forget the ones that are gone.
    std::erase_if(attached_, [this](Attached& view) {
      if (::IsWindow(view.native_view)) {
        return false;
      }
      if (view.blur) {
        RemoveChildViewT(view.blur.ExtractAsDangling());
      }
      return true;
    });
    for (Attached& view : attached_) {
      const HWND hwnd = view.native_view;
      if (view.needs_parent) {
        view.needs_parent = false;
        const LONG_PTR ex_style = ::GetWindowLongPtr(hwnd, GWL_EXSTYLE);
        ::SetWindowLongPtr(hwnd, GWL_EXSTYLE,
                           ex_style & ~(WS_EX_TOOLWINDOW | WS_EX_APPWINDOW));
        ::SetWindowLongPtr(hwnd, GWL_STYLE, WS_CHILD | WS_CLIPSIBLINGS);
        ::SetParent(hwnd, parent);
        ::SetWindowPos(hwnd, nullptr, 0, 0, 0, 0,
                       SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER |
                           SWP_NOACTIVATE | SWP_FRAMECHANGED);
      }
      // Widget coordinates are the HWND's client area, in DIPs.
      const gfx::Rect bounds = gfx::ScaleToRoundedRect(
          ConvertRectToWidget(
              ResolvePlacement(view.options.placement, GetLocalBounds())),
          scale);
      // In attach order, each to the top: later views end above earlier
      // ones, and all of them above Chrome's own child windows.
      ::SetWindowPos(hwnd, HWND_TOP, bounds.x(), bounds.y(), bounds.width(),
                     bounds.height(),
                     SWP_NOACTIVATE | (view.options.visible ? SWP_SHOWWINDOW
                                                            : SWP_HIDEWINDOW));
    }
#endif
  }

  raw_ptr<views::WebView> web_view_ = nullptr;
  std::optional<gfx::Rect> page_rect_;
  std::optional<jaal_placement> page_placement_;
  std::optional<jaal_window_style> style_;
  std::vector<Attached> attached_;
  bool place_pending_ = false;
  base::WeakPtrFactory<PageAreaView> weak_factory_{this};
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
    : browser_(browser),
      frameless_(Engine::Get().creating_window_options().frameless != 0) {
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

  // Sees a mouse press on the page before the page does, to take keyboard
  // focus back from the app's views.
  widget_->GetNativeWindow()->AddPreTargetHandler(this);
#if BUILDFLAG(IS_WIN)
  ui::HWNDSubclass::AddFilterToTarget(views::HWNDForWidget(widget_.get()),
                                      this);
  if (frameless_) {
    // Makes Windows ask for the client area again, now that the filter
    // answers.
    ::SetWindowPos(views::HWNDForWidget(widget_.get()), nullptr, 0, 0, 0, 0,
                   SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE |
                       SWP_FRAMECHANGED);
  }
#endif

  Engine::Get().OnWindowCreated(this);
}

JaalBrowserWindow::~JaalBrowserWindow() {
#if BUILDFLAG(IS_WIN)
  ui::HWNDSubclass::RemoveFilterFromAllTargets(this);
#endif
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

void JaalBrowserWindow::SetPagePlacement(const jaal_placement& placement) {
  page_area_->SetPagePlacement(placement);
  modal_dialog_host_->NotifyPositionRequiresUpdate();
}

jaal_status JaalBrowserWindow::AttachView(void* native_view,
                                          const jaal_view_options& options) {
#if BUILDFLAG(IS_WIN)
  const HWND hwnd = static_cast<HWND>(native_view);
  // A window of another thread would make every placement a cross-thread
  // SendMessage.
  if (!::IsWindow(hwnd) ||
      ::GetWindowThreadProcessId(hwnd, nullptr) != ::GetCurrentThreadId()) {
    return JAAL_ERR_INVALID;
  }
  if (page_area_->HasAttached(hwnd)) {
    return JAAL_ERR_INVALID;
  }
  page_area_->Attach(hwnd, options);
  return JAAL_OK;
#else
  return JAAL_ERR_UNSUPPORTED;
#endif
}

jaal_status JaalBrowserWindow::UpdateView(void* native_view,
                                          const jaal_view_options& options) {
#if BUILDFLAG(IS_WIN)
  const HWND hwnd = static_cast<HWND>(native_view);
  if (!page_area_->HasAttached(hwnd)) {
    return JAAL_ERR_GONE;
  }
  page_area_->Update(hwnd, options);
  return JAAL_OK;
#else
  return JAAL_ERR_UNSUPPORTED;
#endif
}

jaal_status JaalBrowserWindow::DetachView(void* native_view) {
#if BUILDFLAG(IS_WIN)
  const HWND hwnd = static_cast<HWND>(native_view);
  if (!page_area_->HasAttached(hwnd)) {
    return JAAL_ERR_GONE;
  }
  page_area_->Detach(hwnd);
  return JAAL_OK;
#else
  return JAAL_ERR_UNSUPPORTED;
#endif
}

void JaalBrowserWindow::FocusPage() {
  // Posted: moving focus sends WM_KILLFOCUS to the app's view, which must
  // not reach the app while it is inside this call.
  base::SequencedTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE,
      base::BindOnce(
          [](base::WeakPtr<JaalBrowserWindow> window) {
            if (!window) {
              return;
            }
#if BUILDFLAG(IS_WIN)
            const HWND hwnd = views::HWNDForWidget(window->widget_.get());
            if (::IsWindow(hwnd) && ::GetFocus() != hwnd) {
              ::SetFocus(hwnd);
            }
#endif
            if (content::WebContents* contents =
                    window->web_view_->GetWebContents()) {
              contents->Focus();
            }
          },
          weak_factory_.GetWeakPtr()));
}

void JaalBrowserWindow::SetStyle(const jaal_window_style& style) {
  page_area_->SetStyle(style);
}

void JaalBrowserWindow::BeginDrag() {
#if BUILDFLAG(IS_WIN)
  // Posted: the move runs a loop of its own until the button comes up, and
  // the app must be out of its call by then.
  base::SequencedTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE, base::BindOnce(
                     [](HWND hwnd) {
                       if (!::IsWindow(hwnd)) {
                         return;
                       }
                       POINT cursor = {};
                       ::GetCursorPos(&cursor);
                       // The app's view captured the mouse on the press.
                       ::ReleaseCapture();
                       ::SendMessage(hwnd, WM_NCLBUTTONDOWN, HTCAPTION,
                                     MAKELPARAM(cursor.x, cursor.y));
                     },
                     views::HWNDForWidget(widget_.get())));
#endif
}

jaal_window_state JaalBrowserWindow::GetState() const {
  if (IsMinimized()) {
    return JAAL_WINDOW_MINIMIZED;
  }
  if (IsFullscreen()) {
    return JAAL_WINDOW_FULLSCREEN;
  }
  return IsMaximized() ? JAAL_WINDOW_MAXIMIZED : JAAL_WINDOW_NORMAL;
}

void JaalBrowserWindow::SetState(jaal_window_state state) {
  if (state == GetState()) {
    return;
  }
  auto* fullscreen =
      ExclusiveAccessManager::From(browser_)->fullscreen_controller();
  if (IsFullscreen()) {
    // Through Chrome's controller, so tab fullscreen and its own state end
    // too.
    if (fullscreen->IsFullscreenForBrowser()) {
      fullscreen->ToggleBrowserFullscreenMode(/*user_initiated=*/true);
    } else {
      ExitFullscreen();
    }
  }
  switch (state) {
    case JAAL_WINDOW_NORMAL:
      if (IsMinimized() || IsMaximized()) {
        widget_->Restore();
      }
      break;
    case JAAL_WINDOW_MINIMIZED:
      widget_->Minimize();
      break;
    case JAAL_WINDOW_MAXIMIZED:
      widget_->Maximize();
      break;
    case JAAL_WINDOW_FULLSCREEN:
      fullscreen->ToggleBrowserFullscreenMode(/*user_initiated=*/true);
      break;
  }
}

gfx::Size JaalBrowserWindow::GetClientSize() const {
  return page_area_->size();
}

float JaalBrowserWindow::GetScale() const {
  return widget_->GetCompositor()->device_scale_factor();
}

gfx::Rect JaalBrowserWindow::GetContentsBoundsInScreen() const {
  return web_view_->GetBoundsInScreen();
}

void JaalBrowserWindow::OnMouseEvent(ui::MouseEvent* event) {
#if BUILDFLAG(IS_WIN)
  // Windows leaves keyboard focus on a child window when the user clicks its
  // already-active parent, so a click on the page takes focus back from the
  // app's views here.
  if (event->type() == ui::EventType::kMousePressed) {
    const HWND hwnd = views::HWNDForWidget(widget_.get());
    if (::GetFocus() != hwnd) {
      ::SetFocus(hwnd);
    }
  }
#endif
}

#if BUILDFLAG(IS_WIN)
bool JaalBrowserWindow::FilterMessage(HWND hwnd,
                                      UINT message,
                                      WPARAM w_param,
                                      LPARAM l_param,
                                      LRESULT* l_result) {
  switch (message) {
    case WM_NCCALCSIZE:
      if (frameless_ && w_param) {
        auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(l_param);
        const LONG top = params->rgrc[0].top;
        // The default takes the title bar and the sizing borders out of the
        // client area. The borders stay (they give resizing, snapping, the
        // shadow and rounded corners); the title bar's height goes back to
        // the client.
        *l_result = ::DefWindowProc(hwnd, message, w_param, l_param);
        params->rgrc[0].top = top;
        if (::IsZoomed(hwnd)) {
          // A maximized window hangs over the screen's edge by the border's
          // thickness on every side.
          const UINT dpi = ::GetDpiForWindow(hwnd);
          params->rgrc[0].top += ::GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) +
                                 ::GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        }
        return true;
      }
      break;
    case WM_NCHITTEST:
      if (frameless_) {
        // The left, right and bottom sizing borders are outside the client
        // area, where the default finds them. The top one is now inside it.
        LRESULT hit = ::DefWindowProc(hwnd, message, w_param, l_param);
        if (hit == HTCLIENT && !::IsZoomed(hwnd) && !IsFullscreen()) {
          RECT window = {};
          ::GetWindowRect(hwnd, &window);
          const UINT dpi = ::GetDpiForWindow(hwnd);
          const int border = ::GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) +
                             ::GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
          // Screen coordinates, signed: a second monitor can sit at
          // negative ones. (windowsx.h has macros for this, and others that
          // rename IsMinimized and IsMaximized.)
          const int x = static_cast<short>(LOWORD(l_param));
          const int y = static_cast<short>(HIWORD(l_param));
          if (y < window.top + border) {
            hit = x < window.left + 2 * border    ? HTTOPLEFT
                  : x >= window.right - 2 * border ? HTTOPRIGHT
                                                   : HTTOP;
          }
        }
        *l_result = hit;
        return true;
      }
      break;
    case WM_MOUSEWHEEL:
    case WM_MOUSEHWHEEL: {
      // Windows sends the wheel to the window with keyboard focus, which is
      // this one while a page has it, not to the child under the pointer.
      // Over one of the app's views, the wheel is that view's.
      const HWND target = page_area_->AttachedAt(gfx::Point(
          static_cast<short>(LOWORD(l_param)),
          static_cast<short>(HIWORD(l_param))));
      if (target && !forwarding_wheel_) {
        // The view passes a wheel it doesn't want back up to its parent.
        base::AutoReset<bool> forwarding(&forwarding_wheel_, true);
        *l_result = ::SendMessage(target, message, w_param, l_param);
        return true;
      }
      break;
    }
    case WM_PARENTNOTIFY:
      // Chrome made a child window of its own; it starts above the app's
      // views.
      if (LOWORD(w_param) == WM_CREATE) {
        RaiseAttachedViewsLater();
      }
      break;
    case WM_KILLFOCUS:
      // Keyboard focus went to one of the app's views. Chrome's text input
      // (TSF) is bound to the thread, not the window, so without this an IME
      // would keep composing into the page.
      if (page_area_->IsAttachedOrChild(reinterpret_cast<HWND>(w_param))) {
        if (ui::InputMethod* input_method = widget_->GetInputMethod()) {
          input_method->OnBlur();
        }
        app_view_has_focus_ = true;
      }
      break;
    case WM_SETFOCUS:
      if (app_view_has_focus_) {
        app_view_has_focus_ = false;
        if (ui::InputMethod* input_method = widget_->GetInputMethod()) {
          input_method->OnFocus();
        }
      }
      break;
  }
  // Chrome handles everything else as before.
  return false;
}
#endif

void JaalBrowserWindow::RaiseAttachedViewsLater() {
  base::SequencedTaskRunner::GetCurrentDefault()->PostTask(
      FROM_HERE, base::BindOnce(
                     [](base::WeakPtr<JaalBrowserWindow> window) {
                       if (window && window->page_area_) {
                         window->page_area_->RaiseAttached();
                       }
                     },
                     weak_factory_.GetWeakPtr()));
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
  // A page shown for the first time brings a child window of its own.
  RaiseAttachedViewsLater();
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
  // The app's reserved shortcuts never reach the page. Chrome's key path is
  // synchronous, so this is the one place the engine asks the app something
  // from the middle of Chrome's work (docs/engine.md › The C API, rule 2).
  if (event.GetType() == blink::WebInputEvent::Type::kRawKeyDown &&
      Engine::Get().OnPageKeyDown(this, event.windows_key_code,
                                  event.GetModifiers(), /*before_page=*/true)) {
    return content::KeyboardEventProcessingResult::HANDLED;
  }
  return content::KeyboardEventProcessingResult::NOT_HANDLED;
}

bool JaalBrowserWindow::HandleKeyboardEvent(
    const input::NativeWebKeyboardEvent& event) {
  // The page left this key alone, so the app gets it.
  if (event.GetType() == blink::WebInputEvent::Type::kRawKeyDown) {
    Engine::Get().OnPageKeyDown(this, event.windows_key_code,
                                event.GetModifiers(), /*before_page=*/false);
  }
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
  Engine::Get().OnWindowStateChanged(this);
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
    Engine::Get().OnWindowStateChanged(this);
  }
}

void JaalBrowserWindow::OnWidgetActivationChanged(views::Widget* widget,
                                                  bool active) {
  if (modal_dialog_host_) {
    Engine::Get().OnWindowStateChanged(this);
  }
}

void JaalBrowserWindow::OnWidgetShowStateChanged(views::Widget* widget) {
  if (modal_dialog_host_) {
    Engine::Get().OnWindowStateChanged(this);
  }
}

}  // namespace jaal
