import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const renderState = vi.hoisted(() => ({ hidden: false, reduced: false }));
vi.mock('../render', () => ({
  useMotionOff: () => renderState.reduced,
  useRenderHidden: () => renderState.hidden,
}));
import { AlchemistCore } from './AlchemistCore';
import { AlchemistField } from './AlchemistField';
import { resolveStoredTheme, THEMES } from '../../store/themeStore';

beforeEach(() => { renderState.hidden = false; renderState.reduced = false; });

describe('Alchemist replacement for the persisted Ophanim slot', () => {
  it('preserves saved selections and exposes the new name', () => {
    expect(resolveStoredTheme('ophanim')).toBe('ophanim');
    expect(THEMES.find(t => t.id === 'ophanim')?.label).toBe('Alchemist');
    expect(THEMES.some(t => t.label === 'Ophanim')).toBe(false);
  });
  it('renders the exact v4 sprite with no nested button in previews', () => {
    const html = renderToStaticMarkup(<AlchemistCore active={false} interactive={false} size={104} />);
    expect(html).toContain('exact-v4/original.webp');
    expect(html).toContain('alchemist-potion-frame');
    expect(html).toContain('data-mood="rest"');
    expect(html).not.toContain('<button');
  });
  it('keeps the accessible protection control and prioritizes faults', () => {
    const html = renderToStaticMarkup(<AlchemistCore active busy scanning alarm onClick={() => {}} />);
    expect(html.match(/<button/g)).toHaveLength(1);
    expect(html).toContain('data-phase="fault"');
    expect(html).toContain('data-mood="rest"');
    expect(html).toContain('Отключить защиту');
    expect(html).toContain('disabled=""');
  });
  it.each([
    [{ active: false }, 'idle', 'rest'],
    [{ active: true }, 'focused', 'ready'],
    [{ active: false, busy: true }, 'engaging', 'brew'],
    [{ active: true, scanning: true }, 'scanning', 'brew'],
  ] as const)('maps %j to %s / %s', (props, phase, mood) => {
    const html = renderToStaticMarkup(<AlchemistCore {...props} />);
    expect(html).toContain(`data-phase="${phase}"`);
    expect(html).toContain(`data-mood="${mood}"`);
  });
  it('honors pause/reduced motion and releases hidden sprites', () => {
    expect(renderToStaticMarkup(<AlchemistCore active paused />)).toContain('data-motion="still"');
    renderState.reduced = true;
    expect(renderToStaticMarkup(<AlchemistCore active />)).toContain('data-motion="still"');
    expect(renderToStaticMarkup(<AlchemistField />)).toContain('data-paused="true"');
    renderState.hidden = true;
    expect(renderToStaticMarkup(<AlchemistCore active />)).not.toContain('<canvas');
    expect(renderToStaticMarkup(<AlchemistField />)).not.toContain('workshop.webp');
  });
  it('uses the approved workshop effects without a second cat or lab controls', () => {
    const html = renderToStaticMarkup(<AlchemistField />);
    expect(html).toContain('scene-v1/workshop.webp');
    expect(html.match(/class="room-flower-stem /g)).toHaveLength(3);
    expect(html).toContain('room-bookmark-tip');
    expect(html).toContain('room-moth-route');
    expect(html).not.toContain('<button');
    expect(html).not.toContain('exact-v4/original.webp');
  });
});
