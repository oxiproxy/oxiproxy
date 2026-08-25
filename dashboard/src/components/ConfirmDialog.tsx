import { AlertTriangle, Info, OctagonAlert } from 'lucide-react';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from './ui/alert-dialog';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  variant?: 'danger' | 'warning' | 'info';
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({
  open,
  title,
  message,
  confirmText = '确定',
  cancelText = '取消',
  variant = 'danger',
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const config = {
    danger: { icon: OctagonAlert, iconClass: 'bg-destructive/10 text-destructive', actionClass: 'bg-destructive hover:bg-destructive/90' },
    warning: { icon: AlertTriangle, iconClass: 'bg-amber-100 text-amber-700 dark:bg-amber-950 dark:text-amber-300', actionClass: 'bg-amber-600 text-white hover:bg-amber-700' },
    info: { icon: Info, iconClass: 'bg-primary/10 text-primary', actionClass: '' },
  }[variant];
  const Icon = config.icon;

  return (
    <AlertDialog open={open} onOpenChange={(nextOpen) => !nextOpen && onCancel()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <div className="flex items-start gap-3 pr-6 text-left">
            <span className={`flex size-10 shrink-0 items-center justify-center rounded-md ${config.iconClass}`}>
              <Icon className="size-5" aria-hidden="true" />
            </span>
            <div className="space-y-1">
              <AlertDialogTitle>{title}</AlertDialogTitle>
              <AlertDialogDescription>{message}</AlertDialogDescription>
            </div>
          </div>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={onCancel}>{cancelText}</AlertDialogCancel>
          <AlertDialogAction className={config.actionClass} onClick={onConfirm}>{confirmText}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
