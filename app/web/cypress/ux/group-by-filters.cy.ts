/// <reference types="cypress" />
///<reference path="../global.d.ts"/>

const env = (key: string) => {
  const valueFromCypress = Cypress.env(key);
  if (typeof valueFromCypress === 'string') {
    return valueFromCypress;
  }

  const metaEnv = (import.meta as unknown as { env?: Record<string, unknown> }).env;
  const fallback = metaEnv?.[key];
  return typeof fallback === 'string' ? fallback : undefined;
};

const SI_WORKSPACE_URL = env('VITE_SI_WORKSPACE_URL');
const SI_WORKSPACE_ID = env('VITE_SI_WORKSPACE_ID');

const groupByOptions = [
  { label: 'Diff Status', value: 'diffstatus', testId: 'group-by-option-diff' },
  { label: 'Qualification Status', value: 'qualificationstatus',testId: 'group-by-option-qualification'},
  { label: 'Upgradeable', value: 'upgradeable', testId: 'group-by-option-upgrade' },
  { label: 'Schema Name', value: 'schemaname', testId: 'group-by-option-schema-name' },
  { label: 'Resource', value: 'resource', testId: 'group-by-option-resource' },
];

describe('UX Group By Dropdown', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for Group By tests');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    // make sure the explore grid is rendered before continuing
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 10000,
    }).should('exist');
  });

  groupByOptions.forEach(({ label, value, testId }) => {
    it(`selecting ${label} updates URL query`, () => {
      // open dropdown
      cy.contains('button', 'Group by').click();

      // select option
      cy.get(`[data-testid="${testId}"]`).click();

      // assert URL contains expected query parameters
      cy.url().should('match', new RegExp(`/h\\?grid=1&groupBy=${value}$`));
    });
  });
});
