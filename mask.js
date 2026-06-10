import sharp from 'sharp';
import { execSync } from 'child_process';

function polarPoint(cx, cy, radius, degrees) {
  const radians = (degrees * Math.PI) / 180;
  return {
    x: cx + radius * Math.sin(radians),
    y: cy - radius * Math.cos(radians),
  };
}

function arcPath(cx, cy, radius, startDegrees, endDegrees) {
  const start = polarPoint(cx, cy, radius, startDegrees);
  const end = polarPoint(cx, cy, radius, endDegrees);
  const largeArc = Math.abs(endDegrees - startDegrees) > 180 ? 1 : 0;
  return `M ${start.x.toFixed(2)} ${start.y.toFixed(2)} A ${radius} ${radius} 0 ${largeArc} 1 ${end.x.toFixed(2)} ${end.y.toFixed(2)}`;
}

async function processIcon() {
  const width = 1024;
  const height = 1024;
  const visibleSize = 840;
  const inset = Math.round((width - visibleSize) / 2);
  const macIconRadius = Math.round(visibleSize * 0.225);
  const center = width / 2;
  const ringRadius = 255;
  const progressStartDegrees = -8;
  const progressEndDegrees = 236;

  // The visible rounded-square area intentionally leaves transparent padding
  // inside the 1024px canvas. That keeps the icon optically aligned in the Dock.
  const iconSvg = Buffer.from(
    `<svg width="${width}" height="${height}" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id="bg" x1="0" y1="${inset}" x2="0" y2="${inset + visibleSize}" gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color="#202124"/>
          <stop offset="0.52" stop-color="#111214"/>
          <stop offset="1" stop-color="#050506"/>
        </linearGradient>
        <radialGradient id="bg-light" cx="50%" cy="22%" r="70%">
          <stop offset="0" stop-color="#ffffff" stop-opacity="0.12"/>
          <stop offset="0.42" stop-color="#ffffff" stop-opacity="0.035"/>
          <stop offset="1" stop-color="#ffffff" stop-opacity="0"/>
        </radialGradient>
        <linearGradient id="ring" x1="300" y1="220" x2="740" y2="790" gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color="#ffffff"/>
          <stop offset="0.44" stop-color="#f4f4f4"/>
          <stop offset="0.72" stop-color="#cfcfcf"/>
          <stop offset="1" stop-color="#8e8e8e"/>
        </linearGradient>
        <linearGradient id="ring-edge" x1="280" y1="245" x2="760" y2="800" gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color="#ffffff" stop-opacity="0.75"/>
          <stop offset="1" stop-color="#000000" stop-opacity="0.3"/>
        </linearGradient>
        <linearGradient id="track" x1="280" y1="230" x2="760" y2="820" gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color="#53565b"/>
          <stop offset="0.5" stop-color="#33363a"/>
          <stop offset="1" stop-color="#202226"/>
        </linearGradient>
        <radialGradient id="inner-disc" cx="50%" cy="34%" r="72%">
          <stop offset="0" stop-color="#23262a"/>
          <stop offset="0.55" stop-color="#141619"/>
          <stop offset="1" stop-color="#0a0b0d"/>
        </radialGradient>
        <filter id="soft-shadow" x="-25%" y="-25%" width="150%" height="150%">
          <feDropShadow dx="0" dy="20" stdDeviation="22" flood-color="#000000" flood-opacity="0.48"/>
        </filter>
        <filter id="inner-glow" x="-20%" y="-20%" width="140%" height="140%">
          <feGaussianBlur stdDeviation="5" result="blur"/>
          <feComposite in="SourceGraphic" in2="blur" operator="over"/>
        </filter>
      </defs>
      <clipPath id="shape">
        <rect
          x="${inset}"
          y="${inset}"
          width="${visibleSize}"
          height="${visibleSize}"
          rx="${macIconRadius}"
          ry="${macIconRadius}"
        />
      </clipPath>
      <g clip-path="url(#shape)">
        <rect x="${inset}" y="${inset}" width="${visibleSize}" height="${visibleSize}" fill="url(#bg)"/>
        <rect x="${inset}" y="${inset}" width="${visibleSize}" height="${visibleSize}" fill="url(#bg-light)"/>
        <circle
          cx="${center}"
          cy="${center}"
          r="${ringRadius - 50}"
          fill="url(#inner-disc)"
          opacity="0.92"
        />
        <path d="${arcPath(center, center, ringRadius, progressStartDegrees, progressEndDegrees)}"
          fill="none"
          stroke="#000000"
          stroke-width="86"
          stroke-linecap="round"
          opacity="0.42"
          transform="translate(0 16)"
        />
        <circle
          cx="${center}"
          cy="${center}"
          r="${ringRadius}"
          fill="none"
          stroke="url(#track)"
          stroke-width="68"
          opacity="0.95"
        />
        <circle
          cx="${center}"
          cy="${center}"
          r="${ringRadius}"
          fill="none"
          stroke="#ffffff"
          stroke-width="18"
          opacity="0.08"
        />
        <path d="${arcPath(center, center, ringRadius, progressStartDegrees, progressEndDegrees)}"
          fill="none"
          stroke="url(#ring)"
          stroke-width="66"
          stroke-linecap="round"
          filter="url(#soft-shadow)"
        />
        <path d="${arcPath(center, center, ringRadius, progressStartDegrees, progressEndDegrees)}"
          fill="none"
          stroke="url(#ring-edge)"
          stroke-width="28"
          stroke-linecap="round"
          opacity="0.42"
          filter="url(#inner-glow)"
        />
        <rect
          x="${inset + 1}"
          y="${inset + 1}"
          width="${visibleSize - 2}"
          height="${visibleSize - 2}"
          rx="${macIconRadius - 1}"
          ry="${macIconRadius - 1}"
          fill="none"
          stroke="#ffffff"
          stroke-width="2"
          opacity="0.08"
        />
      </g>
    </svg>`
  );

  await sharp(iconSvg)
    .png()
    .toFile('app-icon.png');

  await sharp(iconSvg)
    .png()
    .toFile('app-icon-transparent.png');
    
  console.log('Created app-icon-transparent.png');

  // Automatically regenerate all Tauri icons from the transparent png
  console.log('Regenerating all Tauri app icons...');
  try {
    execSync('bun run tauri icon app-icon-transparent.png', { stdio: 'inherit' });
    console.log('Successfully regenerated all app icons.');
  } catch (err) {
    console.error('Failed to run tauri icon generator:', err);
  }

  // Create a template icon for the system status bar (tray)
  // macOS tray icons should be white with transparent background
  // and named *Template.png for macOS to auto-tint it (dark/light mode).
  const traySvg = Buffer.from(
    `<svg width="32" height="32" xmlns="http://www.w3.org/2000/svg">
      <circle cx="16" cy="16" r="12" fill="none" stroke="white" stroke-width="4"/>
      <circle cx="16" cy="16" r="2" fill="white"/>
    </svg>`
  );

  await sharp(traySvg)
    .png()
    .toFile('src-tauri/icons/trayTemplate.png');
    
  await sharp(traySvg)
    .resize(64, 64) // @2x size
    .png()
    .toFile('src-tauri/icons/trayTemplate@2x.png');

  console.log('Created trayTemplate.png and trayTemplate@2x.png');
}

processIcon().catch(console.error);
