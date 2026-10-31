
import type { ReactNode, SVGProps } from 'react';

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, 'children'> {
  size?: number;
}

function Icon({ size = 16, children, ...rest }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      {children}
    </svg>
  );
}

export function HomeIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5.5 9.2V20a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V9.2" />
    </Icon>
  );
}

export function RefreshIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M20 12A8 8 0 1 1 12 4" />
      <path d="M15.6 4 12 2.3v3.4Z" fill="currentColor" stroke="none" />
    </Icon>
  );
}

export function CogIcon(props: IconProps) {
  const teeth = [0, 45, 90, 135, 180, 225, 270, 315];

  return (
    <Icon {...props}>
      {teeth.map((angle) => (
        <rect
          key={angle}
          x="10.6"
          y="1.4"
          width="2.8"
          height="4"
          rx="1"
          fill="currentColor"
          stroke="none"
          transform={`rotate(${angle} 12 12)`}
        />
      ))}
      <circle cx="12" cy="12" r="7.4" />
      <circle cx="12" cy="12" r="2.8" />
    </Icon>
  );
}

export function CloseIcon(props: IconProps) {
  return (
    <Icon {...props}>
      <path d="M6 6l12 12M18 6L6 18" />
    </Icon>
  );
}
