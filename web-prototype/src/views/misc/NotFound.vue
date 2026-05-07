<template>
  <div class="notfound">
    <div class="notfound__code">404</div>
    <div class="notfound__title">页面走丢了</div>
    <div class="notfound__desc">请核对 URL 或返回工作台。</div>
    <el-button type="primary" @click="$router.push('/dashboard')">返回工作台</el-button>
  </div>
</template>

<script setup lang="ts"></script>

<style scoped>
.notfound {
  height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  background: var(--color-canvas);
}
.notfound__code {
  font-size: 96px;
  font-weight: 700;
  letter-spacing: -0.03em;
  background: linear-gradient(135deg, #0F766E, #06B6D4);
  -webkit-background-clip: text;
  -webkit-text-fill-color: transparent;
  line-height: 1;
}
.notfound__title {
  font-size: 20px;
  font-weight: 600;
  color: var(--color-ink);
}
.notfound__desc {
  font-size: 14px;
  color: var(--color-ink-subtle);
  margin-bottom: 12px;
}
</style>
