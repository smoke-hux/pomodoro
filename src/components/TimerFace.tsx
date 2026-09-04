import type { Phase, TimerFace as TimerFaceId } from "../types";

/**
 * Five instruments for one reading.
 *
 * The faces form an abstraction gradient rather than a set of skins: digits are
 * exact to the second, ring and bar trade precision for proportion, pips make
 * the remainder countable, and words drop numerals entirely. Choosing a face
 * changes how the time is *perceived*, not how it is decorated — which is why
 * each one dominates its own visual and demotes everything else.
 *
 * Every face exposes the same value to assistive technology through one
 * <output aria-label>, so the choice is purely visual and never costs a
 * screen-reader user information.
 */

interface TimerFaceProps {
  face: TimerFaceId;
  phase: Phase;
  remainingSeconds: number;
  durationSeconds: number;
  phaseLabel: string;
}

const ONES = [
  "zero", "one", "two", "three", "four", "five", "six", "seven", "eight",
  "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen",
  "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS = ["", "", "twenty", "thirty", "forty", "fifty"];

function spell(value: number): string {
  if (value < 20) return ONES[value];
  const tens = TENS[Math.floor(value / 10)];
  const ones = value % 10;
  return ones ? `${tens}-${ONES[ones]}` : tens;
}

export function clockText(seconds: number) {
  const safe = Math.max(0, Math.ceil(seconds));
  return `${Math.floor(safe / 60)
    .toString()
    .padStart(2, "0")}:${(safe % 60).toString().padStart(2, "0")}`;
}

/**
 * Deliberately vague above ten minutes and exact below one.
 *
 * Precision is what makes a clock urgent — watching a number tick is the
 * opposite of calm. This face rounds to the nearest five while there is plenty
 * of time and sharpens only as the interval actually runs out, which is the one
 * moment the exact figure matters.
 */
export function wordsText(seconds: number): string {
  const safe = Math.max(0, Math.ceil(seconds));
  if (safe === 0) return "the interval is over";
  if (safe < 60) return "less than a minute left";

  const minutes = Math.round(safe / 60);
  if (minutes > 60) return "over an hour left";
  if (minutes === 60) return "an hour left";
  if (minutes === 1) return "about a minute left";
  if (minutes <= 10) return `about ${spell(minutes)} minutes left`;

  // Rounding can push 58 up to 60, which has no tens word — say the hour.
  const rounded = Math.round(minutes / 5) * 5;
  if (rounded >= 60) return "an hour left";
  return `about ${spell(rounded)} minutes left`;
}

/** Polar to cartesian with 0° at twelve o'clock, so angles read like a clock. */
function polar(radius: number, degrees: number): [number, number] {
  const radians = ((degrees - 90) * Math.PI) / 180;
  return [50 + radius * Math.cos(radians), 50 + radius * Math.sin(radians)];
}

/** A pie wedge from twelve o'clock, swept clockwise. */
function wedgePath(radius: number, sweepDegrees: number): string {
  // A 360° sweep would put both arc endpoints on the same coordinate, which SVG
  // renders as nothing at all — the one angle that must not be drawn literally.
  const sweep = Math.min(359.99, Math.max(0, sweepDegrees));
  const [x1, y1] = polar(radius, 0);
  const [x2, y2] = polar(radius, sweep);
  const largeArc = sweep > 180 ? 1 : 0;
  return `M 50 50 L ${x1} ${y1} A ${radius} ${radius} 0 ${largeArc} 1 ${x2} ${y2} Z`;
}

export function TimerFace({
  face,
  phase,
  remainingSeconds,
  durationSeconds,
  phaseLabel,
}: TimerFaceProps) {
  const elapsed = Math.max(0, durationSeconds - remainingSeconds);
  const fraction = durationSeconds > 0 ? Math.min(1, Math.max(0, elapsed / durationSeconds)) : 0;
  const minutesLeft = Math.ceil(Math.max(0, remainingSeconds) / 60);

  // One <output> carries the value for assistive tech on every face, so the
  // visual below it can be as abstract as it likes.
  const label = `${minutesLeft} ${minutesLeft === 1 ? "minute" : "minutes"} remaining, ${phaseLabel}`;

  if (face === "words") {
    return (
      <output className="timer-face face-words" aria-label={label}>
        <span className="words-line">{wordsText(remainingSeconds)}</span>
      </output>
    );
  }

  if (face === "ring") {
    // 44 keeps the stroke inside a 100-unit box at the widths used below.
    const radius = 44;
    const circumference = 2 * Math.PI * radius;
    return (
      <output className="timer-face face-ring" aria-label={label}>
        <svg viewBox="0 0 100 100" role="presentation" focusable="false">
          <circle className="ring-track" cx="50" cy="50" r={radius} />
          <circle
            className="ring-progress"
            cx="50"
            cy="50"
            r={radius}
            strokeDasharray={circumference}
            strokeDashoffset={circumference * fraction}
          />
        </svg>
        <span className="ring-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  if (face === "bar") {
    return (
      <output className="timer-face face-bar" aria-label={label}>
        <span className="bar-shell" aria-hidden="true">
          <span
            className="bar-fill"
            style={{ transform: `scaleX(${1 - fraction})` }}
          />
        </span>
        <span className="bar-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  if (face === "pips") {
    // One pip per minute reads as a tally you can count. Past an hour that
    // becomes a wall of dots, so each pip absorbs five minutes instead and the
    // legend below says so rather than leaving the scale to guesswork.
    const totalMinutes = Math.max(1, Math.ceil(durationSeconds / 60));
    const perPip = totalMinutes > 60 ? 5 : 1;
    const totalPips = Math.ceil(totalMinutes / perPip);
    const litPips = Math.min(totalPips, Math.ceil(minutesLeft / perPip));

    return (
      <output className="timer-face face-pips" aria-label={label}>
        <span className="pip-grid" aria-hidden="true">
          {Array.from({ length: totalPips }, (_, index) => (
            <i key={index} className={index < litPips ? "pip lit" : "pip"} />
          ))}
        </span>
        <span className="pip-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
          {perPip > 1 ? <em> · one mark is five minutes</em> : null}
        </span>
      </output>
    );
  }

  if (face === "analog") {
    // The technique was named after a wind-up kitchen timer, and this is that
    // timer: a wound wedge that unwinds anticlockwise as the interval burns
    // down, with a single hand riding its trailing edge.
    const remainingFraction = 1 - fraction;
    const sweep = remainingFraction * 360;
    const [handX, handY] = polar(34, sweep);
    return (
      <output className="timer-face face-analog" aria-label={label}>
        <svg viewBox="0 0 100 100" role="presentation" focusable="false">
          <circle className="dial-plate" cx="50" cy="50" r="46" />
          {Array.from({ length: 12 }, (_, index) => {
            const [tx1, ty1] = polar(46, index * 30);
            const [tx2, ty2] = polar(index % 3 === 0 ? 38 : 42, index * 30);
            return (
              <line
                key={index}
                className={index % 3 === 0 ? "dial-tick major" : "dial-tick"}
                x1={tx1}
                y1={ty1}
                x2={tx2}
                y2={ty2}
              />
            );
          })}
          {sweep > 0 ? <path className="dial-wedge" d={wedgePath(34, sweep)} /> : null}
          <line className="dial-hand" x1="50" y1="50" x2={handX} y2={handY} />
          <circle className="dial-pin" cx="50" cy="50" r="3" />
        </svg>
      </output>
    );
  }

  if (face === "vessel") {
    // Time as a quantity that drains rather than a number that decrements.
    return (
      <output className="timer-face face-vessel" aria-label={label}>
        <span className="vessel-shell" aria-hidden="true">
          <span
            className="vessel-fill"
            style={{ transform: `scaleY(${1 - fraction})` }}
          />
        </span>
        <span className="vessel-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  if (face === "arc") {
    // A 180° gauge: half the ink of the ring, and the readout sits in the
    // hollow the semicircle leaves behind rather than below it.
    const radius = 40;
    const length = Math.PI * radius;
    return (
      <output className="timer-face face-arc" aria-label={label}>
        <svg viewBox="0 0 100 62" role="presentation" focusable="false">
          <path className="arc-track" d="M 10 50 A 40 40 0 0 1 90 50" />
          <path
            className="arc-progress"
            d="M 10 50 A 40 40 0 0 1 90 50"
            strokeDasharray={length}
            strokeDashoffset={length * fraction}
          />
        </svg>
        <span className="arc-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  if (face === "blocks") {
    // Twelve fixed segments regardless of interval length: unlike pips these
    // are proportional, so a 5-minute break and a 50-minute focus both read at
    // the same glance-weight.
    const TOTAL = 12;
    const lit = Math.ceil((1 - fraction) * TOTAL);
    return (
      <output className="timer-face face-blocks" aria-label={label}>
        <span className="block-row" aria-hidden="true">
          {Array.from({ length: TOTAL }, (_, index) => (
            <i key={index} className={index < lit ? "block lit" : "block"} />
          ))}
        </span>
        <span className="block-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  if (face === "orbit") {
    // One dot completing a single revolution per interval. Position alone
    // carries the reading, which makes it the quietest of the graphic faces.
    const [dotX, dotY] = polar(40, fraction * 360);
    return (
      <output className="timer-face face-orbit" aria-label={label}>
        <svg viewBox="0 0 100 100" role="presentation" focusable="false">
          <circle className="orbit-path" cx="50" cy="50" r="40" />
          <circle className="orbit-dot" cx={dotX} cy={dotY} r="5" />
        </svg>
        <span className="orbit-readout" aria-hidden="true">
          {clockText(remainingSeconds)}
        </span>
      </output>
    );
  }

  return (
    <output className={`timer-face face-digits phase-${phase}`} aria-label={label}>
      {clockText(remainingSeconds)}
    </output>
  );
}
