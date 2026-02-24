'use client';

/**
 * Pure presentational atmospheric background components.
 * Used for ambient visual effects (orbital lines, radial glow, particle field).
 */

export function OrbitalLines() {
  return (
    <svg
      className="pointer-events-none absolute inset-0 h-full w-full [mask-image:radial-gradient(circle_at_50%_46%,transparent_0%,transparent_40%,black_64%,black_100%)]"
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 1000 1000"
      preserveAspectRatio="xMidYMid slice"
    >
      <defs>
        <linearGradient id="orbitStroke" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="hsl(var(--atmosphere-orbit-line) / 0.55)" />
          <stop offset="50%" stopColor="hsl(var(--atmosphere-orbit-line) / 0.20)" />
          <stop offset="100%" stopColor="hsl(var(--atmosphere-orbit-line) / 0.55)" />
        </linearGradient>
      </defs>

      {/* New shared center — slightly lower */}
      <g className="al-orbit al-orbit-1" fill="none" stroke="url(#orbitStroke)" strokeWidth="1" opacity="0.55">
        <ellipse cx="520" cy="490" rx="540" ry="175" transform="rotate(-9 520 550)" />
      </g>

      <g className="al-orbit al-orbit-2" fill="none" stroke="url(#orbitStroke)" strokeWidth="0.85" opacity="0.45">
        <ellipse cx="520" cy="502" rx="490" ry="160" transform="rotate(-9 520 562)" />
      </g>

      <g className="al-orbit al-orbit-3" fill="none" stroke="url(#orbitStroke)" strokeWidth="0.75" opacity="0.38">
        <ellipse cx="520" cy="515" rx="440" ry="145" transform="rotate(-9 520 575)" />
      </g>

      {/* Outer halo ring */}
      <g className="al-orbit al-orbit-4" fill="none" stroke="url(#orbitStroke)" strokeWidth="0.7" opacity="0.26">
        <ellipse cx="520" cy="470" rx="600" ry="195" transform="rotate(-9 520 530)" />
      </g>
    </svg>
  );
}

export function RadialGlow() {
  return (
    <div className="pointer-events-none absolute inset-0">
      {/* LEFT SOURCE (main) */}
      <div
        className="absolute top-[30%] left-[-4%] -translate-x-1/2 -translate-y-1/2 rounded-full"
        style={{
          width: 'clamp(720px, 92vw, 1500px)',
          height: 'clamp(720px, 92vw, 1500px)',
          background: 'radial-gradient(circle, hsl(var(--signal-glow) / 0.14) 0%, transparent 66%)',
          filter: 'blur(26px)',
          willChange: 'transform',
          transform: 'translateZ(0)',
        }}
      />
      <div
        className="absolute top-[26%] left-[0%] -translate-x-1/2 -translate-y-1/2 rounded-full"
        style={{
          width: 'clamp(420px, 58vw, 980px)',
          height: 'clamp(420px, 58vw, 980px)',
          background: 'radial-gradient(circle, hsl(var(--signal-glow) / 0.12) 0%, transparent 62%)',
          filter: 'blur(16px)',
          willChange: 'transform',
          transform: 'translateZ(0)',
        }}
      />
      <div
        className="absolute top-[20%] left-[4%] -translate-x-1/2 -translate-y-1/2 rounded-full"
        style={{
          width: 'clamp(220px, 28vw, 520px)',
          height: 'clamp(220px, 28vw, 520px)',
          background: 'radial-gradient(circle, hsl(var(--signal-glow) / 0.10) 0%, transparent 48%)',
          filter: 'blur(10px)',
          willChange: 'transform',
          transform: 'translateZ(0)',
        }}
      />

      {/* BOTTOM-RIGHT KICKER (secondary) */}
      <div
        className="absolute top-[78%] left-[90%] -translate-x-1/2 -translate-y-1/2 rounded-full"
        style={{
          width: 'clamp(520px, 100vw, 1100px)',
          height: 'clamp(520px, 100vw, 1100px)',
          background: 'radial-gradient(circle, hsl(var(--signal-glow) / 0.18) 0%, transparent 68%)',
          filter: 'blur(24px)',
          willChange: 'transform',
          transform: 'translateZ(0)',
        }}
      />
    </div>
  );
}

import { useMemo, useSyncExternalStore } from 'react';

function mulberry32(seed: number) {
  return function () {
    let t = (seed += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const mobileQuery = typeof window !== 'undefined' ? window.matchMedia('(max-width: 768px)') : null;

function subscribeMobile(cb: () => void) {
  mobileQuery?.addEventListener('change', cb);
  return () => mobileQuery?.removeEventListener('change', cb);
}

function getIsMobile() {
  return mobileQuery?.matches ?? false;
}

function getIsMobileServer() {
  return false;
}

export function ParticleField({ count = 64 }: { count?: number }) {
  const particles = useMemo(() => {
    const rand = mulberry32(12345);
    return Array.from({ length: count }, () => {
      const size = 2 + rand() * 2.2; // 2px–4.2px
      const opacity = 0.1 + rand() * 0.22; // visible but soft
      const twinkle = 6 + rand() * 10; // seconds
      const delay = -rand() * 10; // desync
      return {
        left: `${rand() * 100}%`,
        top: `${rand() * 100}%`,
        size,
        opacity,
        twinkle,
        delay,
      };
    });
  }, [count]);

  return (
    <div className="pointer-events-none absolute inset-0">
      {particles.map((p, i) => (
        <div
          key={i}
          className="al-particle absolute rounded-full"
          style={{
            left: p.left,
            top: p.top,
            width: p.size,
            height: p.size,
            opacity: p.opacity,
            background: 'hsl(var(--atmosphere-particle))',
            filter: 'blur(0.3px)',
            animationDuration: `${p.twinkle}s`,
            animationDelay: `${p.delay}s`,
          }}
        />
      ))}
    </div>
  );
}

export function Atmosphere() {
  const isMobile = useSyncExternalStore(subscribeMobile, getIsMobile, getIsMobileServer);

  return (
    <div className="pointer-events-none fixed inset-0 overflow-hidden" style={{ contain: 'strict' }}>
      <OrbitalLines />
      <RadialGlow />
      {!isMobile && <ParticleField />}
    </div>
  );
}
