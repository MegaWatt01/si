import { tw } from "../../utils/tw-utils";

import colorsJson from "../../tailwind/tailwind_customization/colors.json";
import { useTheme } from "./theme_tools";

export const COLOR_PALETTE = colorsJson;

const TONES = {
  action: {
    colorHex: COLOR_PALETTE.action[500],
    bgColorClassLight: tw`bg-action-500`,
    bgColorClassDark: tw`bg-action-300`,
    textColorClassLight: tw`text-action-500`,
    textColorClassDark: tw`text-action-300`,
    borderColorClassLight: tw`border-action-500`,
    borderColorClassDark: tw`border-action-300`,
  },
  info: {
    colorHex: COLOR_PALETTE.action[500],
    bgColorClass: tw`bg-action-500`,
    textColorClassLight: tw`text-action-500`,
    textColorClassDark: tw`text-action-300`,
    borderColorClassLight: tw`border-action-500`,
    borderColorClassDark: tw`border-action-300`,
  },
  destructive: {
    colorHex: COLOR_PALETTE.destructive[500],
    bgColorClass: tw`bg-destructive-500`,
    textColorClass: tw`text-destructive-500`,
    borderColorClass: tw`border-destructive-500`,
  },
  error: {
    colorHex: COLOR_PALETTE.destructive[500],
    bgColorClass: tw`bg-destructive-500`,
    textColorClass: tw`text-destructive-500`,
    borderColorClass: tw`border-destructive-500`,
  },
  success: {
    colorHex: COLOR_PALETTE.success[500],
    bgColorClass: tw`bg-success-500`,
    textColorClass: tw`text-success-500`,
    borderColorClass: tw`border-success-500`,
  },
  warning: {
    colorHex: COLOR_PALETTE.warning[500],
    bgColorClass: tw`bg-warning-500`,
    textColorClass: tw`text-warning-500`,
    borderColorClass: tw`border-warning-500`,
  },
  neutral: {
    colorHex: COLOR_PALETTE.neutral[500],
    bgColorClass: tw`bg-neutral-500`,
    textColorClass: tw`text-neutral-500`,
    borderColorClass: tw`border-neutral-500`,
  },
  empty: {
    colorHex: "#00000000",
    bgColorClass: tw`bg-transparent`,
    textColorClass: tw`text-transparent border-transparent bg-transparent border-0 border-hidden opacity-0`,
    borderColorClass: tw`border-transparent`,
  },
  shade: {
    colorHexLight: COLOR_PALETTE.shade[0],
    colorHexDark: COLOR_PALETTE.shade[100],
    bgColorClassLight: tw`bg-shade-0`,
    bgColorClassDark: tw`bg-shade-100`,
    textColorClassLight: tw`text-shade-100`,
    textColorClassDark: tw`text-shade-0`,
    borderColorClassLight: tw`bg-shade-0`,
    borderColorClassDark: tw`bg-shade-100`,
  },
};

export type Tones = keyof typeof TONES;

export function getToneBgColorClass(tone: Tones) {
  const { theme } = useTheme();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const toneSettings = TONES[tone] as any;
  if (theme.value === "dark")
    return toneSettings.bgColorClassDark || toneSettings.bgColorClass;
  return toneSettings.bgColorClassLight || toneSettings.bgColorClass;
}

export function getToneTextColorClass(tone: Tones) {
  const { theme } = useTheme();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const toneSettings = TONES[tone] as any;

  if (theme?.value === "dark") {
    return toneSettings?.textColorClassDark ?? toneSettings.textColorClass;
  }

  return toneSettings?.textColorClassLight ?? toneSettings.textColorClass;
}

export function getToneBorderColorClass(tone: Tones) {
  const { theme } = useTheme();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const toneSettings = TONES[tone] as any;
  if (theme.value === "dark")
    return toneSettings.borderColorClassDark || toneSettings.borderColorClass;
  return toneSettings.borderColorClassLight || toneSettings.borderColorClass;
}

export function getToneColorHex(tone: Tones) {
  const { theme } = useTheme();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const toneSettings = TONES[tone] as any;
  if (theme.value === "dark")
    return toneSettings.colorHexDark || toneSettings.colorHex;
  return toneSettings.colorHexLight || toneSettings.colorHex;
}
