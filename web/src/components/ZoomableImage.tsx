import { useEffect, useRef } from "react";
import { TransformComponent, TransformWrapper, type ReactZoomPanPinchRef } from "react-zoom-pan-pinch";

interface Props {
  src: string;
  alt: string;
  className?: string;
  /** Called when the image is clicked without dragging (e.g. open a lightbox). */
  onOpen?: () => void;
  testId?: string;
}

interface GestureEvent extends UIEvent {
  scale: number;
  clientX: number;
  clientY: number;
}

/**
 * Photo viewer with zoom & pan: mouse wheel, trackpad pinch (ctrl+wheel in Chrome/Firefox,
 * gesture events in Safari), two-finger pinch on touch screens, drag to pan,
 * double-click / double-tap to zoom, and explicit +/−/reset buttons.
 */
export default function ZoomableImage({ src, alt, className, onOpen, testId }: Props) {
  const ref = useRef<ReactZoomPanPinchRef | null>(null);
  const wrapper = useRef<HTMLDivElement>(null);
  const startScale = useRef(1);

  // Safari trackpad pinch emits non-standard gesture events instead of ctrl+wheel.
  useEffect(() => {
    const el = wrapper.current;
    if (!el) return;
    const onStart = (e: Event) => {
      e.preventDefault();
      startScale.current = ref.current?.state.scale ?? 1;
    };
    const onChange = (e: Event) => {
      e.preventDefault();
      const g = e as GestureEvent;
      const api = ref.current;
      if (!api) return;
      const { positionX, positionY, scale } = api.state;
      const next = Math.min(8, Math.max(1, startScale.current * g.scale));
      const rect = el.getBoundingClientRect();
      const px = g.clientX - rect.left;
      const py = g.clientY - rect.top;
      const ratio = next / scale;
      api.setTransform(px - (px - positionX) * ratio, py - (py - positionY) * ratio, next, 0);
    };
    el.addEventListener("gesturestart", onStart);
    el.addEventListener("gesturechange", onChange);
    return () => {
      el.removeEventListener("gesturestart", onStart);
      el.removeEventListener("gesturechange", onChange);
    };
  }, []);

  // Reset zoom when the photo changes.
  useEffect(() => {
    ref.current?.resetTransform(0);
  }, [src]);

  return (
    <div className={`zoomable ${className ?? ""}`} ref={wrapper} data-testid={testId}>
      <TransformWrapper
        ref={ref}
        minScale={1}
        maxScale={8}
        centerOnInit
        limitToBounds
        wheel={{ step: 0.15 }}
        pinch={{ step: 5 }}
        doubleClick={{ mode: "toggle", step: 1.5 }}
        panning={{ velocityDisabled: true }}
      >
        {({ zoomIn, zoomOut, resetTransform }) => (
          <>
            <TransformComponent wrapperClass="zoomable-wrapper" contentClass="zoomable-content">
              <img
                src={src}
                alt={alt}
                draggable={false}
                className="zoomable-img"
                onClick={onOpen}
                data-testid={testId ? `${testId}-img` : undefined}
              />
            </TransformComponent>
            <div className="zoom-controls" onClick={(e) => e.stopPropagation()}>
              <button type="button" aria-label="Zoom in" onClick={() => zoomIn(0.6)}>
                +
              </button>
              <button type="button" aria-label="Zoom out" onClick={() => zoomOut(0.6)}>
                −
              </button>
              <button type="button" aria-label="Reset zoom" onClick={() => resetTransform()}>
                ⤢
              </button>
            </div>
          </>
        )}
      </TransformWrapper>
    </div>
  );
}
