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

gfx::Size JaalModalDialogHost::GetMaximumDialogSize() {
  return window_->GetContentsBoundsInScreen().size();
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
  // Coordinates are relative to the host view, which is the whole window.
  gfx::Rect contents_bounds = window_->GetContentsBoundsInScreen();
  contents_bounds -= window_->GetBounds().OffsetFromOrigin();
  return gfx::Point(
      contents_bounds.x() + (contents_bounds.width() - size.width()) / 2,
      contents_bounds.y() - 10);
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
