import React from 'react';

interface AppLogoProps {
  className?: string;
  size?: number;
}

export const AppLogo: React.FC<AppLogoProps> = ({ className = 'w-5 h-5', size = 20 }) => (
  <img
    src="/icon-192.png"
    width={size}
    height={size}
    className={className}
    alt=""
    aria-hidden="true"
    draggable={false}
  />
);
