import { defineStore } from "pinia";
import * as _ from "lodash-es";
import { addStoreHooks } from "@si/vue-lib/pinia";
import { posthog } from "@/utils/posthog";

// translation from store key to posthog feature flag name
const FLAG_MAPPING = {
  // STORE_FLAG_NAME: "posthogFlagName",
  MODULES_TAB: "modules_tab",
  IS_ACTIONS_V2: "actions_v2", // TODO - THIS SHOULD BE REMOVED ONCE ACTIONS V1 IS GONE
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

        this.IS_ACTIONS_V2 = false; // TODO -  THIS SHOULD BE REMOVED ONCE ACTIONS V1 IS GONE

        // Make sure to remove the override before committing your code!
      },
    }),
  )();
}
