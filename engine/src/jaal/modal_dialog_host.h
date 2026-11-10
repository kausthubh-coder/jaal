#ifndef JAAL_MODAL_DIALOG_HOST_H_
#define JAAL_MODAL_DIALOG_HOST_H_

#include "base/memory/raw_ptr.h"
#include "base/observer_list.h"
#include "components/web_modal/web_contents_modal_dialog_host.h"

namespace jaal {

class JaalBrowserWindow;

// Where Chrome's tab-modal dialogs go (HTTP auth, JavaScript dialogs,
// beforeunload): centred over the page, hanging from its top edge. Copied from
// WebUIBrowserModalDialogHost, which only differs in its window class.
class JaalModalDialogHost : public web_modal::WebContentsModalDialogHost {
 public:
  explicit JaalModalDialogHost(JaalBrowserWindow* window);
  JaalModalDialogHost(const JaalModalDialogHost&) = delete;
  JaalModalDialogHost& operator=(const JaalModalDialogHost&) = delete;
  ~JaalModalDialogHost() override;

  // web_modal::WebContentsModalDialogHost:
  void NotifyPositionRequiresUpdate() override;
  gfx::Size GetMaximumDialogSize() override;
  gfx::NativeView GetHostView() const override;
  bool ShouldActivateDialog() const override;
  gfx::Point GetDialogPosition(const gfx::Size& size) override;
  void AddObserver(web_modal::ModalDialogHostObserver* observer) override;
  void RemoveObserver(web_modal::ModalDialogHostObserver* observer) override;

 private:
  const raw_ptr<JaalBrowserWindow> window_;
  base::ObserverList<web_modal::ModalDialogHostObserver> observers_;
};

}  // namespace jaal

#endif  // JAAL_MODAL_DIALOG_HOST_H_
