/**
 * Gera um "mapa de cidade" estilizado e determinístico (mesma semente no
 * servidor e no navegador, para a hidratação bater). Cada quadra recebe uma
 * nota de 0 a 1 que pinta a camada de dados revelada pela lupa do hero.
 */
export interface MapBlock {
  x: number;
  y: number;
  w: number;
  h: number;
  score: number;
  park: boolean;
}

export interface MapPin {
  x: number;
  y: number;
  label: string;
  value: string;
}

export const MAP_WIDTH = 1600;
export const MAP_HEIGHT = 1000;

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function cuts(total: number, min: number, max: number, rand: () => number): number[] {
  const result = [0];
  let acc = 0;
  while (acc < total) {
    acc += min + rand() * (max - min);
    result.push(Math.min(acc, total));
  }
  return result;
}

export function buildCityMap(seed = 7): MapBlock[] {
  const rand = mulberry32(seed);
  const street = 14;
  const cols = cuts(MAP_WIDTH, 70, 150, rand);
  const rows = cuts(MAP_HEIGHT, 60, 120, rand);
  const blocks: MapBlock[] = [];

  // "Bairros" quentes e frios para a nota variar de forma orgânica.
  const hotspots = [
    { x: 1150, y: 300, v: 0.95 },
    { x: 400, y: 750, v: 0.2 },
    { x: 900, y: 650, v: 0.7 },
    { x: 1400, y: 820, v: 0.35 },
    { x: 250, y: 200, v: 0.6 },
  ];

  for (let r = 0; r < rows.length - 1; r++) {
    for (let c = 0; c < cols.length - 1; c++) {
      const x = cols[c] + street / 2;
      const y = rows[r] + street / 2;
      const w = cols[c + 1] - cols[c] - street;
      const h = rows[r + 1] - rows[r] - street;
      if (w < 20 || h < 20) continue;

      const cx = x + w / 2;
      const cy = y + h / 2;
      let weight = 0;
      let value = 0;
      for (const spot of hotspots) {
        const d = Math.hypot(cx - spot.x, cy - spot.y);
        const k = 1 / (1 + (d / 260) ** 2);
        weight += k;
        value += k * spot.v;
      }
      const score = Math.min(1, Math.max(0, value / weight + (rand() - 0.5) * 0.18));

      blocks.push({ x, y, w, h, score, park: rand() > 0.93 });
    }
  }
  return blocks;
}

export const MAP_PINS: MapPin[] = [
  { x: 1150, y: 300, label: 'Segurança', value: '9,1' },
  { x: 880, y: 640, label: 'Escolas', value: '12' },
  { x: 1380, y: 600, label: 'Metrô', value: '4 min' },
  { x: 560, y: 380, label: 'Hospitais', value: '3' },
  { x: 1250, y: 860, label: 'Parques', value: '2,4 km²' },
  { x: 300, y: 720, label: 'Ruído', value: '61 dB' },
];

/** Escala de cor da nota: verde (bom) → amarelo → vermelho (atenção). */
export function scoreColor(score: number): string {
  const stops = [
    [250, 47, 58],
    [255, 196, 87],
    [6, 214, 160],
  ];
  const t = score * (stops.length - 1);
  const i = Math.min(stops.length - 2, Math.floor(t));
  const f = t - i;
  const [r, g, b] = stops[i].map((v, k) => Math.round(v + (stops[i + 1][k] - v) * f));
  return `rgb(${r}, ${g}, ${b})`;
}
