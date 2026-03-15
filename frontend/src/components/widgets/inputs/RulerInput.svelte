<script lang="ts">
	import { createEventDispatcher, onMount } from "svelte";
	import type { Editor } from "@graphite/editor";

	const dispatch = createEventDispatcher<{
		dragStart: undefined;
		dragMove: { isHorizontal: boolean; isEnd: boolean; newValue: number };
		translateMove: { isHorizontal: boolean; newValue: number };
		dragEnd: undefined;
	}>();

	const RULER_THICKNESS = 16;
	const MAJOR_MARK_THICKNESS = 16;
	const MINOR_MARK_THICKNESS = 6;
	const MICRO_MARK_THICKNESS = 3;
	const TAU = 2 * Math.PI;

	type RulerDirection = "Horizontal" | "Vertical";

	export let direction: RulerDirection = "Vertical";
	export let originX: number;
	export let originY: number;
	export let numberInterval: number;
	export let majorMarkSpacing: number;
	export let minorDivisions = 5;
	export let microDivisions = 2;
	export let tilt: number = 0;
	export let lineStart: number | null = null;
	export let lineEnd: number | null = null;
	export let originMarkerPos: number | null = null;

	let rulerInput: HTMLDivElement | undefined;
	let rulerLength = 0;
	let svgBounds = { width: "0px", height: "0px" };

	type Axis = { sign: number; vec: [number, number] };

	$: axes = computeAxes(tilt);
	$: isHorizontal = direction === "Horizontal";
	$: trackedAxis = isHorizontal ? axes.horiz : axes.vert;
	$: otherAxis = isHorizontal ? axes.vert : axes.horiz;
	$: stretchFactor = 1 / (isHorizontal ? trackedAxis.vec[0] : trackedAxis.vec[1]);
	$: stretchedSpacing = majorMarkSpacing * stretchFactor;
	$: effectiveOrigin = computeEffectiveOrigin(direction, originX, originY, otherAxis);
	$: svgPath = computeSvgPath(direction, effectiveOrigin, stretchedSpacing, stretchFactor, minorDivisions, microDivisions, rulerLength, otherAxis);
	$: svgTexts = computeSvgTexts(direction, effectiveOrigin, stretchedSpacing, numberInterval, rulerLength, trackedAxis, otherAxis);

	function computeAxes(tilt: number): { horiz: Axis; vert: Axis } {
		const normTilt = ((tilt % TAU) + TAU) % TAU;
		const octant = Math.floor((normTilt + Math.PI / 4) / (Math.PI / 2)) % 4;

		const [c, s] = [Math.cos(tilt), Math.sin(tilt)];
		const posX: Axis = { sign: 1, vec: [c, s] };
		const posY: Axis = { sign: 1, vec: [-s, c] };
		const negX: Axis = { sign: -1, vec: [-c, -s] };
		const negY: Axis = { sign: -1, vec: [s, -c] };

		if (octant === 0) return { horiz: posX, vert: posY };
		if (octant === 1) return { horiz: negY, vert: posX };
		if (octant === 2) return { horiz: negX, vert: negY };
		return { horiz: posY, vert: negX };
	}

	function computeEffectiveOrigin(direction: RulerDirection, ox: number, oy: number, otherAxis: Axis): number {
		const [vx, vy] = otherAxis.vec;
		return direction === "Horizontal" ? ox - oy * (vx / vy) : oy - ox * (vy / vx);
	}

	function computeSvgPath(
		direction: RulerDirection,
		effectiveOrigin: number,
		stretchedSpacing: number,
		stretchFactor: number,
		minorDivisions: number,
		microDivisions: number,
		rulerLength: number,
		otherAxis: Axis,
	): string {
		const adaptive = stretchFactor > 1.3 ? { minor: minorDivisions, micro: 1 } : { minor: minorDivisions, micro: microDivisions };
		const divisions = stretchedSpacing / adaptive.minor / adaptive.micro;
		const majorMarksFrequency = adaptive.minor * adaptive.micro;
		const shiftedOffsetStart = mod(effectiveOrigin, stretchedSpacing) - stretchedSpacing;

		const [vx, vy] = otherAxis.vec;
		const flip = direction === "Horizontal" ? (vy > 0 ? -1 : 1) : vx > 0 ? -1 : 1;
		const [dx, dy] = [vx * flip, vy * flip];
		const [sxBase, syBase] = direction === "Horizontal" ? [0, RULER_THICKNESS] : [RULER_THICKNESS, 0];

		let path = "";
		let i = 0;
		for (let location = shiftedOffsetStart; location < rulerLength + RULER_THICKNESS; location += divisions) {
			let length;
			if (i % majorMarksFrequency === 0) length = MAJOR_MARK_THICKNESS;
			else if (i % adaptive.micro === 0) length = MINOR_MARK_THICKNESS;
			else length = MICRO_MARK_THICKNESS;
			i += 1;

			const destination = Math.round(location) + 0.5;
			const [sx, sy] = direction === "Horizontal" ? [destination, syBase] : [sxBase, destination];
			path += `M${sx},${sy}l${dx * length},${dy * length} `;
		}

		return path;
	}

	function computeSvgTexts(
		direction: RulerDirection,
		effectiveOrigin: number,
		stretchedSpacing: number,
		numberInterval: number,
		rulerLength: number,
		trackedAxis: Axis,
		otherAxis: Axis,
	): { transform: string; text: string }[] {
		const isVertical = direction === "Vertical";

		const [vx, vy] = otherAxis.vec;
		const flip = isVertical ? (vx > 0 ? -1 : 1) : vy > 0 ? -1 : 1;
		const tipOffsetX = vx * flip * MAJOR_MARK_THICKNESS;
		const tipOffsetY = vy * flip * MAJOR_MARK_THICKNESS;

		const shiftedOffsetStart = mod(effectiveOrigin, stretchedSpacing) - stretchedSpacing;
		const increments = Math.round((shiftedOffsetStart - effectiveOrigin) / stretchedSpacing);
		let labelNumber = increments * numberInterval * trackedAxis.sign;

		const results: { transform: string; text: string }[] = [];

		for (let location = shiftedOffsetStart; location < rulerLength; location += stretchedSpacing) {
			const destination = Math.round(location);
			const x = isVertical ? 9 : destination + 2 + tipOffsetX;
			const y = isVertical ? destination + 1 + tipOffsetY : 9;

			let transform = `translate(${x} ${y})`;
			if (isVertical) transform += " rotate(270)";

			const num = Math.abs(labelNumber) < 1e-9 ? 0 : labelNumber;
			const text = numberInterval >= 1 ? `${num}` : num.toFixed(Math.abs(Math.log10(numberInterval))).replace(/\.0+$/, "");

			results.push({ transform, text });
			labelNumber += numberInterval * trackedAxis.sign;
		}

		return results;
	}

	export function resize() {
		if (!rulerInput) return;

		const isVertical = direction === "Vertical";
		const newLength = isVertical ? rulerInput.clientHeight : rulerInput.clientWidth;
		const roundedUp = (Math.floor(newLength / stretchedSpacing) + 2) * stretchedSpacing;

		if (roundedUp !== rulerLength) {
			rulerLength = roundedUp;
			const thickness = `${RULER_THICKNESS}px`;
			const length = `${roundedUp}px`;
			svgBounds = isVertical ? { width: thickness, height: length } : { width: length, height: thickness };
		}
	}

	// Modulo function that works for negative numbers, unlike the JS `%` operator
	function mod(n: number, m: number): number {
		const remainder = n % m;
		return Math.floor(remainder >= 0 ? remainder : remainder + m);
	}

	function createDragHandler(initialPos: number, onMove: (newValue: number) => void) {
		return (event: PointerEvent) => {
			if (event.button !== 0) return;
			event.stopPropagation();
			(event.target as HTMLElement).setPointerCapture(event.pointerId);

			dispatch("dragStart");

			const startX = event.clientX;
			const startY = event.clientY;

			const onPointerMove = (moveEvent: PointerEvent) => {
				const delta = isHorizontal ? moveEvent.clientX - startX : moveEvent.clientY - startY;
				onMove(initialPos + delta);
			};

			const onPointerUp = () => {
				window.removeEventListener("pointermove", onPointerMove);
				window.removeEventListener("pointerup", onPointerUp);
				dispatch("dragEnd");
			};

			window.addEventListener("pointermove", onPointerMove);
			window.addEventListener("pointerup", onPointerUp);
		};
	}

	function onMarkerPointerDown(event: PointerEvent, isEnd: boolean) {
		const initialPos = isEnd ? lineEnd : lineStart;
		if (initialPos === null) return;

		const handler = createDragHandler(initialPos, (newValue) => {
			dispatch("dragMove", { isHorizontal, isEnd, newValue });
		});
		handler(event);
	}

	function onLinePointerDown(event: PointerEvent) {
		const initialPos = lineStart !== null && lineEnd !== null ? (lineStart + lineEnd) / 2 : 0;
		const handler = createDragHandler(initialPos, (newValue) => {
			dispatch("translateMove", { isHorizontal, newValue });
		});
		handler(event);
	}

	onMount(resize);
</script>

<div class={`ruler-input ${direction.toLowerCase()}`} bind:this={rulerInput}>
	<svg style:width={svgBounds.width} style:height={svgBounds.height}>
		<path d={svgPath} />
		{#each svgTexts as svgText}
			<text transform={svgText.transform}>{svgText.text}</text>
		{/each}
		{#if lineStart !== null && lineEnd !== null}
			<line
				x1={isHorizontal ? lineStart : RULER_THICKNESS / 2}
				y1={isHorizontal ? RULER_THICKNESS / 2 : lineStart}
				x2={isHorizontal ? lineEnd : RULER_THICKNESS / 2}
				y2={isHorizontal ? RULER_THICKNESS / 2 : lineEnd}
				stroke="transparent"
				stroke-width="8px"
				style:cursor="move"
				on:pointerdown={onLinePointerDown}
			/>
			<line
				x1={isHorizontal ? lineStart : RULER_THICKNESS / 2}
				y1={isHorizontal ? RULER_THICKNESS / 2 : lineStart}
				x2={isHorizontal ? lineEnd : RULER_THICKNESS / 2}
				y2={isHorizontal ? RULER_THICKNESS / 2 : lineEnd}
				stroke="#00A8FF"
				stroke-width="1px"
				style:pointer-events="none"
			/>
			<rect
				x={isHorizontal ? lineStart - 4 : RULER_THICKNESS / 2 - 4}
				y={isHorizontal ? RULER_THICKNESS / 2 - 4 : lineStart - 4}
				width="8"
				height="8"
				fill="transparent"
				stroke="#00A8FF"
				stroke-width="1px"
				style:cursor={isHorizontal ? "ew-resize" : "ns-resize"}
				on:pointerdown={(e) => onMarkerPointerDown(e, false)}
			/>
			<rect
				x={isHorizontal ? lineEnd - 4 : RULER_THICKNESS / 2 - 4}
				y={isHorizontal ? RULER_THICKNESS / 2 - 4 : lineEnd - 4}
				width="8"
				height="8"
				fill="transparent"
				stroke="#00A8FF"
				stroke-width="1px"
				style:cursor={isHorizontal ? "ew-resize" : "ns-resize"}
				on:pointerdown={(e) => onMarkerPointerDown(e, true)}
			/>
		{/if}
		{#if originMarkerPos !== null}
			<circle
				cx={isHorizontal ? originMarkerPos : RULER_THICKNESS / 2}
				cy={isHorizontal ? RULER_THICKNESS / 2 : originMarkerPos}
				r="2.5"
				fill="none"
				stroke="#FFD500"
				stroke-width="1px"
			/>
		{/if}
	</svg>
</div>

<style lang="scss" global>
	.ruler-input {
		flex: 1 1 100%;
		background: var(--color-2-mildblack);
		overflow: hidden;
		position: relative;
		box-sizing: border-box;

		&.horizontal {
			height: 16px;
			border-bottom: 1px solid var(--color-5-dullgray);
		}

		&.vertical {
			width: 16px;
			border-right: 1px solid var(--color-5-dullgray);

			svg text {
				text-anchor: end;
			}
		}

		svg {
			position: absolute;

			path {
				stroke-width: 1px;
				stroke: var(--color-5-dullgray);
			}

			text {
				font-size: 12px;
				fill: var(--color-8-uppergray);
			}
		}
	}
</style>
