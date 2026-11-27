import { RefObject, useEffect, useRef } from 'react';

const dialogStack: HTMLElement[] = [];

interface UseDialogFocusOptions<TDialog extends HTMLElement> {
  isOpen: boolean;
  onClose?: () => void;
  initialFocusRef?: RefObject<HTMLElement>;
  dialogRef?: RefObject<TDialog>;
}

/**
 * Provides the shared modal keyboard contract: focus on open, trap Tab, close
 * on Escape, and return focus to the trigger when the dialog closes.
 */
export const useDialogFocus = <TDialog extends HTMLElement = HTMLElement>({
  isOpen,
  onClose,
  initialFocusRef,
  dialogRef: providedDialogRef,
}: UseDialogFocusOptions<TDialog>): RefObject<TDialog> => {
  const localDialogRef = useRef<TDialog>(null);
  const dialogRef = providedDialogRef ?? localDialogRef;
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!isOpen) return;

    const previousActiveElement = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    if (!dialog) return;
    dialogStack.push(dialog);
    const focusTarget = initialFocusRef?.current ?? dialogRef.current;
    const focusFrame = window.requestAnimationFrame(() => {
      focusTarget?.focus();
    });

    const handleKeyDown = (event: KeyboardEvent) => {
      if (dialogStack[dialogStack.length - 1] !== dialog) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        onCloseRef.current?.();
        return;
      }

      if (event.key !== 'Tab' || !dialogRef.current) return;
      const focusable = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
        )
      ).filter((element) => !element.closest('[hidden], [inert], [aria-hidden="true"]')
        && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden');

      if (focusable.length === 0) {
        event.preventDefault();
        dialogRef.current.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (document.activeElement === dialog || !dialog.contains(document.activeElement)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => {
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener('keydown', handleKeyDown);
      const wasTop = dialogStack[dialogStack.length - 1] === dialog;
      const index = dialogStack.lastIndexOf(dialog);
      if (index >= 0) dialogStack.splice(index, 1);
      if (wasTop && previousActiveElement?.isConnected) previousActiveElement.focus();
    };
  }, [dialogRef, initialFocusRef, isOpen]);

  return dialogRef;
};
