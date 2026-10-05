import type { SVGAttributes } from "react"

// The kit's mark: a gear moving left to right, two motion streaks behind it. One colour,
// currentColor, so it follows the text colour of wherever it sits (light and dark).
// Replace it with your own logo. public/icon.svg and public/icon.png are the favicon: the
// kit's colour version (orange gear, two purple bars) on a dark tile, for 16 px browser tabs.
export default function AppLogoIcon(props: SVGAttributes<SVGElement>) {
  return (
    <svg
      fill="currentColor"
      height="32"
      viewBox="0 0 24 24"
      width="32"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <path d="M0.2 7Q3.3 5.4 8.4 5.4L8.4 8.6Q3.3 8.6 0.2 7ZM0.2 17Q3.3 15.4 8.4 15.4L8.4 18.6Q3.3 18.6 0.2 17Z" />
      <path
        fillRule="evenodd"
        d="M21.00 9.54L23.42 9.49A8.60 8.60 0 0 1 23.42 14.51L21.00 14.46A6.30 6.30 0 0 1 20.23 15.79L20.23 15.79L21.49 17.87A8.60 8.60 0 0 1 17.13 20.38L15.97 18.25A6.30 6.30 0 0 1 14.43 18.25L14.43 18.25L13.27 20.38A8.60 8.60 0 0 1 8.91 17.87L10.17 15.79A6.30 6.30 0 0 1 9.40 14.46L9.40 14.46L6.98 14.51A8.60 8.60 0 0 1 6.98 9.49L9.40 9.54A6.30 6.30 0 0 1 10.17 8.21L10.17 8.21L8.91 6.13A8.60 8.60 0 0 1 13.27 3.62L14.43 5.75A6.30 6.30 0 0 1 15.97 5.75L15.97 5.75L17.13 3.62A8.60 8.60 0 0 1 21.49 6.13L20.23 8.21A6.30 6.30 0 0 1 21.00 9.54ZM12.60 12.00A2.60 2.60 0 1 0 17.80 12.00A2.60 2.60 0 1 0 12.60 12.00Z"
      />
    </svg>
  )
}
