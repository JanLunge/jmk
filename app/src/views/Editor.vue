<script setup lang="ts">
import type { Session } from '../lib/session'
defineProps<{ session: Session }>()
</script>

<template>
  <div class="col" style="padding: 40px"><h1>{{ session.meta.name }}</h1></div>
</template>
