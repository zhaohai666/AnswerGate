<script lang="ts">
  /** 冷却页圆形倒计时（数值由 Rust gate://cooldown-tick 事件驱动，前端不自算） */
  export let remaining = 0;
  export let total = 120;

  // 环形进度几何：r=90, 周长 ≈ 565.49
  const RADIUS = 90;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

  $: fraction = total > 0 && remaining >= 0 ? Math.min(remaining / total, 1) : 0;
  $: dashOffset = CIRCUMFERENCE * (1 - fraction);
</script>

<div class="ring-wrap" role="timer" aria-label={`剩余 ${remaining} 秒`}>
  <svg viewBox="0 0 220 220" class="ring">
    <circle cx="110" cy="110" r={RADIUS} class="track" />
    <circle
      cx="110"
      cy="110"
      r={RADIUS}
      class="fill"
      stroke-dasharray={CIRCUMFERENCE}
      stroke-dashoffset={dashOffset}
      transform="rotate(-90 110 110)"
    />
  </svg>
  <div class="number">{remaining}</div>
  <div class="unit">秒</div>
</div>

<style>
  .ring-wrap {
    position: relative;
    width: 220px;
    height: 220px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ring {
    position: absolute;
    inset: 0;
    transform: rotate(0deg);
  }

  circle {
    fill: none;
    stroke-width: 14;
    stroke-linecap: round;
  }

  .track {
    stroke: #dfe5ef;
  }

  .fill {
    stroke: var(--ag-warning);
    transition: stroke-dashoffset 0.9s linear;
  }

  .number {
    font-size: 64px;
    font-weight: 700;
    color: var(--ag-text);
  }

  .unit {
    position: absolute;
    bottom: 34px;
    font-size: 20px;
    color: var(--ag-text-secondary);
  }
</style>
