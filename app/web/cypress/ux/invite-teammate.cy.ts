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

describe('UX Invite Teammate Link', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for invite teammate test');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 60000,
    }).should('exist');
  });

  it('opens the invite a teammate link in new tab', () => {
    const inviteUrl = `https://auth.systeminit.com/workspace/${SI_WORKSPACE_ID}`;

    cy.contains('a', 'Invite a teammate')
      .should('have.attr', 'target', '_blank')
      .invoke('removeAttr', 'target')
      .invoke('attr', 'rel', 'noopener noreferrer')
      .should('have.attr', 'href', inviteUrl);

    cy.contains('a', 'Invite a teammate').click();

    cy.url().should('eq', inviteUrl);
  });
});
