#ifndef JAAL_LOCATION_BAR_H_
#define JAAL_LOCATION_BAR_H_

#include "base/memory/raw_ptr.h"
#include "chrome/browser/ui/location_bar/location_bar.h"
#include "chrome/browser/ui/views/bubble_anchor_util_views.h"

class BrowserWindowInterface;

namespace jaal {

// Chrome's code asks every window for a LocationBar. jaal's address bar is the
// app's, so this answers "nothing here". Copied from WebUIStubLocationBar
// without its WebUI permission chip; permission prompts become the app's
// (docs/engine.md › Who draws what).
class JaalLocationBar : public LocationBar {
 public:
  explicit JaalLocationBar(BrowserWindowInterface* browser);
  JaalLocationBar(const JaalLocationBar&) = delete;
  JaalLocationBar& operator=(const JaalLocationBar&) = delete;
  ~JaalLocationBar() override;

  // LocationBar:
  void FocusLocation(bool is_user_initiated,
                     bool clear_focus_if_failed) override;
  void FocusSearch() override;
  void UpdateFocusBehavior(bool toolbar_visible) override;
  void UpdateContentSettingsIcons() override;
  void SaveStateToContents(content::WebContents* contents) override;
  void Revert() override;
  OmniboxView* GetOmniboxView() override;
  OmniboxPopupView* GetOmniboxPopupView() override;
  OmniboxController* GetOmniboxController() override;
  bool ShouldCloseOmniboxPopup(ui::MouseEvent* event) override;
  ChipController* GetChipController() override;
  content::WebContents* GetWebContents() override;
  LocationBarModel* GetLocationBarModel() override;
  std::optional<bubble_anchor_util::AnchorConfiguration> GetChipAnchor()
      override;
  ui::TrackedElement* GetAnchorOrNull() override;
  BrowserWindowInterface* GetBrowser() override;
  Profile* GetProfile() override;
  void OnChanged() override;
  void UpdateWithoutTabRestore() override;
  bool IsInitialized() const override;
  bool IsVisible() const override;
  bool IsDrawn() const override;
  bool IsFullscreen() const override;
  bool IsEditingOrEmpty() const override;
  bool IsMouseHovered() const override;
  bool IsFocusWithin() const override;
  void InvalidateLayout() override;
  gfx::Rect Bounds() const override;
  gfx::Rect BoundsInScreen() const override;
  gfx::Size MinimumSize() const override;
  gfx::Size PreferredSize() const override;
  void Update(content::WebContents* contents) override;
  void ResetTabState(content::WebContents* contents) override;
  bool HasSecurityStateChanged() override;
  LocationBarTesting* GetLocationBarForTesting() override;
  void AnnounceAlert(const std::u16string& announcement) override;

 private:
  const raw_ptr<BrowserWindowInterface> browser_;
};

}  // namespace jaal

#endif  // JAAL_LOCATION_BAR_H_
