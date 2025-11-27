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

const sortByOptions = [
  { label: 'Latest to oldest', value: '' },
  { label: 'Failing actions', value: 'failingactions' },
  { label: 'Running actions', value: 'runningactions' },
];

describe('UX Sort By Dropdown', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for Sort By tests');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    // make sure the explore grid is rendered before continuing
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 60000,
    }).should('exist');
  });

  sortByOptions.forEach(({ label, value }) => {
    it(`selecting ${label} updates URL query`, () => {
      // open dropdown
      cy.contains('button', 'Sort by').click();

      // select option
      cy.get('.dropdown-menu').contains(label).click();

      // assert URL contains expected query parameters
      const suffix = value ? `/h\\?grid=1&sortBy=${value}$` : `/h\\?grid=1$`;
      cy.url().should('match', new RegExp(suffix));
    });
  });
});
