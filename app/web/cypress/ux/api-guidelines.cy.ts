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

describe('UX API Guidelines Link', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for API guidelines test');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 60000,
    }).should('exist');
  });

  const assertExternalLink = (label: string, suffix: RegExp) => {
    cy.contains('a', label)
      .should('have.attr', 'target', '_blank')
      .invoke('removeAttr', 'target')
      .invoke('attr', 'rel', 'noopener noreferrer')
      .should('have.attr', 'href')
      .and('match', suffix);

    cy.contains('a', label).click();
    cy.url().should('match', suffix);
  };

  it('API guidelines button opens docs link in new tab', () => {
    assertExternalLink('API guidelines', /public-api$/);
  });

  it('How-to guide button opens docs link in new tab', () => {
    assertExternalLink('How-to guide', /how-tos\/?$/);
  });
});
