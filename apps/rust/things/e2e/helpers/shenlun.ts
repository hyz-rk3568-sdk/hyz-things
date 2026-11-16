import { expect, type Page, type Route } from '@playwright/test';

// Shared Question Bank E2E fixtures. Keep request behaviour here explicit:
// callers may handle extra paths first, but every unhandled request fails.
export const shenlunPaperId = '019a3d8c-7f23-7b6e-927d-3c951851d3a6';
export const shenlunQuestionId = '019a3d8e-1b32-7a60-9c83-f34217a6e250';
export const shenlunNextQuestionId = '019a3d8e-1b32-7a60-9c83-f34217a6e251';

export const shenlunCatalog = [{
  id: shenlunPaperId,
  title: '申论真题端到端样卷',
  year: 2025,
  region: '国考',
  variant: '地市级',
  subject: 'shenlun',
  role: 'primary',
  kind: 'exam',
  collection_id: null,
  collection_title: null,
  questions: 2,
}];

export const shenlunPaperView = {
  ...shenlunCatalog[0],
  materials: [{ id: 'material-1', name: '给定资料 1', markdown: '推进基层治理，需要明确责任。' }],
  questions: [
    {
      id: shenlunQuestionId,
      number: 1,
      stem: '请概括基层治理的要求。',
      options_markdown: '',
      materials: ['material-1'],
      module: null,
      category: null,
      topics: [],
      answer_type: 'subjective',
      question_type: 'summary',
      grading_mode: 'ai',
      max_score: 20,
    },
    {
      id: shenlunNextQuestionId,
      number: 2,
      stem: '请提出优化基层服务的对策。',
      options_markdown: '',
      materials: ['material-1'],
      module: null,
      category: null,
      topics: [],
      answer_type: 'subjective',
      question_type: 'measures',
      grading_mode: 'ai',
      max_score: 20,
    },
  ],
};

export const shenlunSourceAnswer = (text: string, sourceHash = 'source-shenlun-v1') => ({
  text,
  provenance: '题库来源（非官方）',
  source_hash: sourceHash,
});

export type ShenlunFavoriteItem = {
  question_id: string;
  paper_id: string;
  paper_title: string;
  year: number;
  region: string;
  number: number;
  module: string | null;
  category: string | null;
};

export type ShenlunCard = {
  id: string;
  source_question_id: string | null;
  source_file: string;
  source_order: number;
  deck_path: string;
  front_markdown: string;
  back_markdown: string;
  tags: string[];
  active: boolean;
  due_at_ms: number | null;
  last_reviewed_at_ms: number | null;
  state: 'new' | 'learning' | 'review' | 'relearning';
  review_count: number;
  lapse_count: number;
  is_new: boolean;
};

export type ShenlunRouteOptions = {
  sourceAnswers?: Record<string, ReturnType<typeof shenlunSourceAnswer>>;
  favorites?: ShenlunFavoriteItem[];
  cards?: ShenlunCard[];
  // Return true when the override fulfilled the request. Returning false lets
  // the shared fixture handle papers, paper views and source answers.
  onRequest?: (route: Route, pathname: string, method: string) => Promise<boolean>;
};

export function shenlunCard(overrides: Partial<ShenlunCard> & Pick<ShenlunCard, 'id'>): ShenlunCard {
  return {
    source_question_id: shenlunQuestionId,
    source_file: '真题练习/申论/归纳概括/错题/cards.md',
    source_order: 0,
    deck_path: '真题练习/申论/归纳概括/错题',
    front_markdown: '卡片正面',
    back_markdown: '卡片背面',
    tags: [],
    active: true,
    due_at_ms: null,
    last_reviewed_at_ms: null,
    state: 'new',
    review_count: 0,
    lapse_count: 0,
    is_new: true,
    ...overrides,
  };
}

export async function installShenlunRoutes(page: Page, options: ShenlunRouteOptions = {}) {
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (options.onRequest && await options.onRequest(route, pathname, method)) return;
    if (pathname.endsWith('/shenlun/papers') && method === 'GET') {
      return route.fulfill({ json: shenlunCatalog });
    }
    if (pathname.endsWith('/shenlun/papers/' + shenlunPaperId) && method === 'GET') {
      return route.fulfill({ json: shenlunPaperView });
    }
    if (pathname.endsWith('/shenlun/favorites') && method === 'GET') {
      return route.fulfill({ json: options.favorites ?? [] });
    }
    const source = pathname.match(/\/shenlun\/questions\/([^/]+)\/source-answer$/);
    if (source && method === 'GET') {
      const answer = options.sourceAnswers?.[source[1]];
      if (answer) return route.fulfill({ json: answer });
      return route.fulfill({ status: 404, json: { error: 'source answer missing' } });
    }
    // Retired attempt/job/confirmation prototypes have no success path.
    throw new Error(`Unexpected Shenlun request: ${method} ${pathname}`);
  });
  await page.route('**/api/v1/study/cards**', route =>
    route.fulfill({ json: { cards: options.cards ?? [] } }));
}

export async function openShenlun(page: Page) {
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('shenlun-catalog')).toBeVisible();
  await page.getByTestId('question-bank-paper-option').click();
  await page.getByTestId('shenlun-start-review').click();
  await expect(page.getByTestId('shenlun-workspace')).toBeVisible();
}

export function watchRetiredShenlunWrites(page: Page) {
  const writes: string[] = [];
  page.on('request', request => {
    const { pathname } = new URL(request.url());
    if (/\/shenlun\/(attempts|jobs)(\/|$)/.test(pathname)) {
      writes.push(`${request.method()} ${pathname}`);
    }
  });
  return writes;
}

export async function scratchpadHasInk(page: Page) {
  return page.getByTestId('calculation-scratchpad-canvas').evaluate(element => {
    const canvas = element as HTMLCanvasElement;
    const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 3; index < pixels.length; index += 4) {
      if (pixels[index] !== 0) return true;
    }
    return false;
  });
}

export async function scratchpadHasStoredInk(page: Page, scope: string) {
  return page.evaluate(scope => new Promise<boolean>(resolve => {
    const request = indexedDB.open('hyz-study-ink');
    request.onerror = () => resolve(false);
    request.onsuccess = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('drafts')) {
        database.close();
        resolve(false);
        return;
      }
      const transaction = database.transaction('drafts', 'readonly');
      const read = transaction.objectStore('drafts').get(scope);
      read.onerror = () => resolve(false);
      read.onsuccess = () => {
        const history = read.result?.history;
        resolve(typeof history === 'string' && JSON.parse(history).length > 0);
        database.close();
      };
    };
  }), scope);
}

export async function drawScratchStroke(page: Page, paneTestId: string) {
  const bounds = await page.getByTestId(paneTestId).boundingBox();
  expect(bounds).not.toBeNull();
  await page.mouse.move(bounds!.x + 24, bounds!.y + 24);
  await page.mouse.down();
  await page.mouse.move(bounds!.x + 68, bounds!.y + 46, { steps: 4 });
  await page.mouse.up();
  await expect.poll(() => scratchpadHasInk(page)).toBe(true);
}

// Drives the OCR EventSource deterministically: tests capture each stream and
// emit snapshot/message events without depending on the native OCR service.
export async function installFakeOcrEventSource(page: Page) {
  await page.addInitScript(() => {
    const sources: Array<{ url: string; source: any }> = [];
    class FakeEventSource extends EventTarget {
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror: ((event: Event) => void) | null = null;
      readyState = 1;
      constructor(public url: string) {
        super();
        if (url.includes('/ocr/') && url.endsWith('/events')) sources.push({ url, source: this });
      }
      close() { this.readyState = 2; }
      emit(name: string, data: string) {
        const event = new MessageEvent(name, { data });
        this.dispatchEvent(event);
        if (name === 'message') this.onmessage?.(event);
      }
    }
    (window as any).__shenlunOcrSources = sources;
    (window as any).__emitShenlunOcr = (runId: string, name: string, data: unknown) => {
      const entry = sources.find(({ url }) => url.endsWith(`/ocr/${runId}/events`));
      if (!entry) throw new Error(`Missing OCR EventSource for ${runId}`);
      entry.source.emit(name, JSON.stringify(data));
    };
    Object.defineProperty(window, 'EventSource', { configurable: true, value: FakeEventSource });
  });
}
