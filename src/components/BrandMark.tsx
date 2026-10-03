/** The app logo (capsule with a sound wave), drawn inline so it stays sharp at any size. */
export function BrandMark({ size = 32, className = '' }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="100 100 824 824"
      className={className}
      role="img"
      aria-label="Penfall"
    >
      <defs>
        <linearGradient id="brand-mark-bg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2a2a2d" />
          <stop offset="1" stopColor="#0b0b0c" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#brand-mark-bg)" />
      <rect x="232" y="392" width="560" height="240" rx="120" fill="#fff" />
      <g fill="#111">
        <rect x="302" y="487" width="36" height="50" rx="18" />
        <rect x="366" y="462" width="36" height="100" rx="18" />
        <rect x="430" y="432" width="36" height="160" rx="18" />
        <rect x="494" y="457" width="36" height="110" rx="18" />
        <rect x="558" y="427" width="36" height="170" rx="18" />
        <rect x="622" y="467" width="36" height="90" rx="18" />
        <rect x="686" y="487" width="36" height="50" rx="18" />
      </g>
    </svg>
  )
}
