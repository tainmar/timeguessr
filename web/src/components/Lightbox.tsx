import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import ZoomableImage from "./ZoomableImage";

interface Props {
  src: string;
  alt: string;
  onClose: () => void;
  onPrev?: () => void;
  onNext?: () => void;
  children?: ReactNode;
}

/** Full-screen zoomable photo with an optional side panel (details, map...). */
export default function Lightbox({ src, alt, onClose, onPrev, onNext, children }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowLeft") onPrev?.();
      if (e.key === "ArrowRight") onNext?.();
    };
    window.addEventListener("keydown", onKey);
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = overflow;
    };
  }, [onClose, onPrev, onNext]);

  return createPortal(
    <div className="lightbox" role="dialog" aria-modal="true" aria-label={alt} data-testid="lightbox">
      <div className="lightbox-photo">
        <ZoomableImage src={src} alt={alt} testId="lightbox-zoom" />
        {onPrev && (
          <button className="lightbox-nav prev" onClick={onPrev} aria-label="Previous photo">
            ‹
          </button>
        )}
        {onNext && (
          <button className="lightbox-nav next" onClick={onNext} aria-label="Next photo">
            ›
          </button>
        )}
      </div>
      {children && <aside className="lightbox-side">{children}</aside>}
      <button className="lightbox-close" onClick={onClose} aria-label="Close" data-testid="lightbox-close">
        ✕
      </button>
    </div>,
    document.body,
  );
}
