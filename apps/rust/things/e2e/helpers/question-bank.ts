import { expect, type Page } from '@playwright/test';
import { loginAsAdmin } from '../fixtures';

// Shared Question Bank entry helpers. Behaviour-specific request mocks stay in
// the specs; only the deterministic fixture paper navigation lives here.
export async function chooseFixturePaper(page: Page) {
  const region = page.getByTestId('question-bank-region').filter({ hasText: '国考' });
  if (!(await region.evaluate(node => (node.closest('details') as HTMLDetailsElement | null)?.open))) {
    await region.click();
  }
  const year = page.getByTestId('question-bank-year').filter({ hasText: '2008' });
  if (!(await year.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) await year.click();
  await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
}

export async function openSheet(page: Page) {
  if (!(await page.getByTestId('training-answer-sheet').isVisible())) {
    await page.getByTestId('training-answer-sheet-toggle').click();
  }
  await expect(page.getByTestId('training-answer-sheet')).toBeVisible();
}

export async function openQuestionBankPaper(page: Page) {
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
}

export async function openFixtureReview(page: Page) {
  await openQuestionBankPaper(page);
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button', { name: '确认交卷' }).click();
}

export function aiRequestInfo(
  request_id: string,
  intent = 'follow_up',
  status = 'complete',
  error: string | null = null,
  remote_uncertain = false,
) {
  return {
    request_id,
    run_id: 'run-1',
    intent,
    status,
    error,
    remote_uncertain,
    new_thread_requested: false,
    user_message_id: null,
    retry_of: null,
  };
}

export async function mockAiRequestStatus(page: Page) {
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*', async route => {
    const request_id = new URL(route.request().url()).pathname.split('/').at(-1)!;
    await route.fulfill({ json: aiRequestInfo(request_id) });
  });
}
