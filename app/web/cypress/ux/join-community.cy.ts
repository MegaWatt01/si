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

describe('UX Join Community Link', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for join community test');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 60000,
    }).should('exist');
  });

  it('opens the join the community link in new tab', () => {
    const joinUrl = 'https://discord.com/invite/system-init';

    cy.contains('a', 'Join the community')
      .should('have.attr', 'target', '_blank')
      .invoke('removeAttr', 'target')
      .invoke('attr', 'rel', 'noopener noreferrer')
      .should('have.attr', 'href', joinUrl);

    cy.contains('a', 'Join the community').click();

    cy.url().should('eq', joinUrl);
  });
});
