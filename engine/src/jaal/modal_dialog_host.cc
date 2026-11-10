#include "jaal/modal_dialog_host.h"

#include "chrome/browser/ui/browser_window/public/global_browser_collection.h"
#include "jaal/browser_window.h"
#include "ui/gfx/native_ui_util.h"

namespace jaal {

JaalModalDialogHost::JaalModalDialogHost(JaalBrowserWindow* window)
    : window_(window) {}

JaalModalDialogHost::~JaalModalDialogHost() {
  observers_.Notify(&web_modal::ModalDialogHostObserver::OnHostDestroying);
}

void JaalModalDialogHost::NotifyPositionRequiresUpdate() {
  observers_.Notify(
      &web_modal::ModalDialogHostObserver::OnPositionRequiresUpdate);
}

namespace {

// How far below the page's top edge a dialog sits, in DIPs. Chrome hangs its
// dialogs from the toolbar; jaal has none, and the page is a card with
// rounded corners, so the dialog sits clear of them.
constexpr int kTopInset = 16;

}  // namespace

gfx::Size JaalModalDialogHost::GetMaximumDialogSize() {
  gfx::Size size = window_->GetContentsBoundsInWidget().size();
  size.Enlarge(0, -2 * kTopInset);
  return size;
}

gfx::NativeView JaalModalDialogHost::GetHostView() const {
  return gfx::GetViewForWindow(window_->GetNativeWindow());
}

bool JaalModalDialogHost::ShouldActivateDialog() const {
  // Same rule as BrowserView's WebContentsModalDialogHostViews.
  return GlobalBrowserCollection::GetInstance()->GetLastActiveBrowser() ==
         window_->browser();
}

gfx::Point JaalModalDialogHost::GetDialogPosition(const gfx::Size& size) {
  // Coordinates are relative to the host view, which is the window's client
  // area. (The window's own bounds include its sizing borders, which would
  // put the dialog off the page's centre by their width.)
  const gfx::Rect page = window_->GetContentsBoundsInWidget();
  return gfx::Point(page.x() + (page.width() - size.width()) / 2,
                    page.y() + kTopInset);
}

void JaalModalDialogHost::AddObserver(
    web_modal::ModalDialogHostObserver* observer) {
  observers_.AddObserver(observer);
}

void JaalModalDialogHost::RemoveObserver(
    web_modal::ModalDialogHostObserver* observer) {
  observers_.RemoveObserver(observer);
}

}  // namespace jaal
