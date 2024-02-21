import { defineStore } from "pinia";
import * as _ from "lodash-es";
import { addStoreHooks } from "@si/vue-lib/pinia";
import { posthog } from "@/utils/posthog";

// translation from store key to posthog feature flag name
const FLAG_MAPPING = {
  // STORE_FLAG_NAME: "posthogFlagName",
  // STORE_FLAG_NAME: "posthogFlagName",
  COPY_PASTE: "copy_paste",
  DONT_BLOCK_ON_ACTIONS: "dont_block_on_actions",
  INVITE_USER: "invite_user",
  JOI_VALIDATIONS: "joi_validations",
  MODULES_TAB: "modules_tab",
  MULTI_VARIANT_EDITING: "multiVariantEditing",
  OVERRIDE_SCHEMA: "override_schema",
  RESIZABLE_PANEL_UPGRADE: "resizable-panel-upgrade",
  SECRETS: "secrets",
  WORKSPACE_BACKUPS: "workspaceBackups",
  SEARCH_FILTERS: "search-filters",
  STATUSBAR_RESOURCE_COUNT: "statusbar_resource_count",
};

type FeatureFlags = keyof typeof FLAG_MAPPING;
const PH_TO_STORE_FLAG_LOOKUP = _.invert(FLAG_MAPPING) as Record<
  string,
  FeatureFlags
>;

export function useFeatureFlagsStore() {
  return addStoreHooks(
    defineStore("feature-flags", {
      // all flags default to false
      state: () => _.mapValues(FLAG_MAPPING, () => false),
      onActivated() {
        posthog.onFeatureFlags((phFlags) => {
          // reset local flags from posthog data
          _.each(phFlags, (phFlag) => {
            const storeFlagKey = PH_TO_STORE_FLAG_LOOKUP[phFlag];
            if (storeFlagKey) {
              this[storeFlagKey as FeatureFlags] = true;
            }
          });
        });
        // You can override feature flags while working on a feature by setting them to true here

        // this.MULTI_VARIANT_EDITING = true;

        // Make sure to remove the override before committing your code!
      },
    }),
  )();
}
