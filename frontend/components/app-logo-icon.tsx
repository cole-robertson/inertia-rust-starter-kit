import { type SVGAttributes, useId } from "react"

// The kit's mark: two Inertia chevrons in front of an orange gear whose inner edge is a crescent
// around them, the ring's ends fading out to the left. Colour, not currentColor: the gradients
// read on light and dark, so put it on the page background (or a bordered `bg-background` tile),
// not on a coloured fill. Size it with className (`size-6`); the viewBox is a little wider than
// tall, and the default preserveAspectRatio centres it in a square box without squashing it.
// The paths match docs/logo/mark.svg. Gradient ids come from useId(), since the mark renders
// several times on a page. Replace it with your own logo. public/icon.svg and public/icon.png
// are the favicon and app icon.
export default function AppLogoIcon(props: SVGAttributes<SVGElement>) {
  const id = useId()
  const gear = `${id}-gear`
  const fade = `${id}-fade`
  const ends = `${id}-ends`
  const top = `${id}-top`
  const bottom = `${id}-bottom`
  const upper = `${id}-upper`
  const lower = `${id}-lower`
  const chevrons =
    "M388.7,301.4 Q382.0,294.0 392.0,294.0 L578.0,294.0 Q588.0,294.0 594.7,301.4 L793.7,522.6 Q800.4,530.0 793.6,537.4 L594.8,754.6 Q588.0,762.0 578.0,762.0 L392.0,762.0 Q382.0,762.0 388.8,754.6 L587.6,537.4 Q594.4,530.0 587.7,522.6 Z M652.7,301.4 Q646.0,294.0 656.0,294.0 L842.0,294.0 Q852.0,294.0 858.7,301.4 L1057.7,522.6 Q1064.4,530.0 1057.6,537.4 L858.8,754.6 Q852.0,762.0 842.0,762.0 L656.0,762.0 Q646.0,762.0 652.8,754.6 L851.6,537.4 Q858.4,530.0 851.7,522.6 Z"

  return (
    <svg
      height="32"
      viewBox="358 88 952 884"
      width="35"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <defs>
        <linearGradient
          id={gear}
          gradientUnits="userSpaceOnUse"
          x1="748"
          y1="112"
          x2="1028"
          y2="948"
        >
          <stop offset="0" stopColor="#FF9636" />
          <stop offset="0.55" stopColor="#F0621F" />
          <stop offset="1" stopColor="#C23414" />
        </linearGradient>
        <linearGradient
          id={fade}
          gradientUnits="userSpaceOnUse"
          x1="650"
          y1="0"
          x2="810"
          y2="0"
        >
          <stop offset="0" stopColor="#fff" stopOpacity="0" />
          <stop offset="1" stopColor="#fff" stopOpacity="1" />
        </linearGradient>
        <mask
          id={ends}
          maskUnits="userSpaceOnUse"
          x="358"
          y="88"
          width="952"
          height="884"
        >
          <rect
            x="358"
            y="88"
            width="952"
            height="884"
            fill={`url(#${fade})`}
          />
          <path
            d="M523,524 a277,277 0 1,0 554,0 a277,277 0 1,0 -554,0 Z"
            fill="#000"
          />
          <path
            d="M868.0,530.0 L643.6,952.0 L604.8,929.0 L568.3,902.3 L534.5,872.4 L503.6,839.4 L476.0,803.6 L452.0,765.4 L431.6,725.1 L415.1,683.0 L402.7,639.6 L394.5,595.1 L390.4,550.2 L390.7,505.0 L395.1,460.0 L403.9,415.7 L416.7,372.4 L433.6,330.5 L454.4,290.4 L478.9,252.4 L506.8,216.9 L538.0,184.2 L572.1,154.6 L608.8,128.4 L647.9,105.7 L688.9,86.8 Z"
            fill="#000"
          />
        </mask>
        <linearGradient
          id={top}
          gradientUnits="userSpaceOnUse"
          x1="0"
          y1="294"
          x2="0"
          y2="530"
        >
          <stop offset="0" stopColor="#9A5CFB" />
          <stop offset="1" stopColor="#6E47F5" />
        </linearGradient>
        <linearGradient
          id={bottom}
          gradientUnits="userSpaceOnUse"
          x1="0"
          y1="530"
          x2="0"
          y2="762"
        >
          <stop offset="0" stopColor="#5A36E0" />
          <stop offset="1" stopColor="#3F22B8" />
        </linearGradient>
        <clipPath id={upper}>
          <rect x="358" y="88" width="952" height="442" />
        </clipPath>
        <clipPath id={lower}>
          <rect x="358" y="530" width="952" height="442" />
        </clipPath>
      </defs>
      <path
        d="M518.0,526.9 L451.3,497.2 L474.0,390.5 L547.0,390.4 L586.7,321.8 L550.2,258.5 L631.2,185.5 L690.4,228.4 L762.8,196.2 L770.4,123.5 L878.9,112.1 L901.5,181.6 L979.1,198.1 L1028.0,143.8 L1122.5,198.4 L1099.9,267.9 L1152.9,326.8 L1224.4,311.6 L1268.8,411.3 L1209.7,454.2 L1218.0,533.1 L1284.7,562.8 L1262.0,669.5 L1189.0,669.6 L1149.3,738.2 L1185.8,801.5 L1104.8,874.5 L1045.6,831.6 L973.2,863.8 L965.6,936.5 L857.1,947.9 L834.5,878.4 L756.9,861.9 L708.0,916.2 L613.5,861.6 L636.1,792.1 L583.1,733.2 L511.6,748.4 L467.2,648.7 L526.3,605.8 Z"
        fill={`url(#${gear})`}
        mask={`url(#${ends})`}
      />
      <path d={chevrons} fill={`url(#${top})`} clipPath={`url(#${upper})`} />
      <path d={chevrons} fill={`url(#${bottom})`} clipPath={`url(#${lower})`} />
    </svg>
  )
}
