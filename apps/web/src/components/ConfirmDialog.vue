<script setup lang="ts">
import { Button } from '@/components/ui/button';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';

withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    description: string;
    confirmLabel?: string;
    pendingLabel?: string;
    pending?: boolean;
  }>(),
  { confirmLabel: 'Delete', pendingLabel: 'Deleting…', pending: false },
);

const emit = defineEmits<{ 'update:open': [boolean]; confirm: [] }>();
</script>

<template>
  <AlertDialog :open="open" @update:open="emit('update:open', $event)">
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>{{ title }}</AlertDialogTitle>
        <AlertDialogDescription>{{ description }}</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel :disabled="pending">Cancel</AlertDialogCancel>
        <!--
          Deliberately a plain Button, not AlertDialogAction: the latter closes
          the dialog as part of its own click handler, so the parent's pending
          item is cleared before `confirm` runs and deletes become no-ops. The
          parent closes the dialog by clearing its item after the request.
        -->
        <Button variant="destructive" :disabled="pending" @click="emit('confirm')">
          {{ pending ? pendingLabel : confirmLabel }}
        </Button>
      </AlertDialogFooter>
    </AlertDialogContent>
  </AlertDialog>
</template>
