import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { AlchemistComposition } from './AlchemistComposition';

describe('hybrid workshop composition', () => {
  it('uses three source flower cutouts with a hidden-background plate and a two-part bookmark', () => {
    const html = renderToStaticMarkup(React.createElement(AlchemistComposition, { paused: false, reference: false, status: 'Колдует', onTogglePause: () => {}, children: null }));
    expect(html.match(/class="room-flower-stem /g)).toHaveLength(3);
    for (const name of ['backing', 'tall', 'left', 'right']) expect(html).toContain(`scene-v2-flora/${name}.webp`);
    expect(html).toContain('class="room-bookmark-ribbon"');
    expect(html).toContain('class="room-bookmark-tip"');
  });
  it('adds a single shelf moth and keeps moving ingredients inside the herb jar', () => {
    const html = renderToStaticMarkup(React.createElement(AlchemistComposition, { paused: false, reference: false, status: 'Колдует', onTogglePause: () => {}, children: null }));
    expect(html.match(/class="room-moth-route"/g)).toHaveLength(1);
    expect(html).toContain('class="room-moth-wings"');
    expect(html.match(/class="room-jar-leaf(?: |")/g)).toHaveLength(3);
    expect(html).toContain('clip-path="url(#workshop-herb-jar)"');
  });
  it('renders moving smoke, clipped bottle bubbles and three fireflies', () => {
    const html = renderToStaticMarkup(React.createElement(AlchemistComposition, { paused: false, reference: false, status: 'Колдует', onTogglePause: () => {}, children: null }));
    expect(html).toContain('viewBox="0 0 1536 1024"');
    expect(html.match(/class="room-smoke-puff/g)).toHaveLength(3);
    expect(html.match(/class="room-liquid-rise/g)).toHaveLength(4);
    expect(html.match(/class="room-firefly(?: |")/g)).toHaveLength(3);
    expect(html).toContain('clip-path="url(#workshop-sage-liquid)"');
    expect(html).toContain('clip-path="url(#workshop-violet-liquid)"');
  });
  it('keeps the original WebP and embeds the existing animated core', () => {
    const html = renderToStaticMarkup(React.createElement(AlchemistComposition, { paused: false, reference: false, status: 'Колдует', onTogglePause: () => {}, children: React.createElement('canvas', { 'data-test-core': 'true' }) }));
    expect(html).toContain('scene-v1/workshop.webp');
    expect(html).toContain('data-test-core="true"');
    expect(html).not.toContain('exact-v4/original.webp');
    expect(html).toContain('data-ambient="true"');
    expect(html).toContain('Пауза сцены');
  });
  it('disables atmosphere in reference mode and exposes paused state', () => {
    const html = renderToStaticMarkup(React.createElement(AlchemistComposition, { paused: true, reference: true, status: 'Размышляет', onTogglePause: () => {}, children: React.createElement('span', null, 'core') }));
    expect(html).toContain('data-ambient="false"');
    expect(html).toContain('data-paused="true"');
    expect(html).toContain('Оживить сцену');
    expect(html).toContain('Размышляет');
  });
});
