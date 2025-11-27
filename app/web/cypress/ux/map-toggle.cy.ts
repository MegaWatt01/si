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

describe('UX Map Toggle', () => {
  beforeEach(() => {
    cy.basicLogin();
    const workspaceUrl = SI_WORKSPACE_URL;
    const workspaceId = SI_WORKSPACE_ID;
    if (!workspaceUrl || !workspaceId) {
      throw new Error('Missing workspace URL or ID for map toggle test');
    }

    cy.visit(`${workspaceUrl}/n/${workspaceId}/auto`);
    cy.get('[data-testid="left-column-new-hotness-explore"]', {
      timeout: 10000,
    }).should('exist');
    cy.contains('Map').click();
  });

  it('switches to map view and updates URL query', () => {
    cy.contains('Grid').should('exist');
    cy.url().should('match', /\/h\?(.*&)?map=1(?!.*grid=1)/);
    cy.contains('Grid').click();
    cy.url().should('match', /\/h\?(.*&)?grid=1(?!.*map=1)/);
    cy.contains('Map').click();
    cy.url().should('match', /\/h\?(.*&)?map=1(?!.*grid=1)/);
  });

  it('toggles minimap visibility', () => {
    cy.get('[data-testid="hide-minimap"]').trigger('mouseover');
    cy.contains('Hide Minimap').should('be.visible');
    cy.get('[data-testid="hide-minimap"]').trigger('mouseout').click();
    cy.get('[data-testid="hide-minimap"]').trigger('mouseover');
    cy.contains('Show Minimap').should('be.visible');
    cy.get('[data-testid="hide-minimap"]').trigger('mouseout');
  });

  it('adjusts zoom via zoom out button', () => {
    cy.get('[data-testid="map-zoom-display"]')
      .invoke('text')
      .then((text) => {
        const initial = parseInt(text, 10);
        cy.get('[data-testid="map-zoom-out"]').click();
        cy.get('[data-testid="map-zoom-display"]')
          .invoke('text')
          .then((newText) => {
            const next = parseInt(newText, 10);
            expect(next < initial).to.equal(true);
          });
      });
  });

  it('adjusts zoom via zoom in button', () => {
    cy.get('[data-testid="map-zoom-display"]')
      .invoke('text')
      .then((text) => {
        const initial = parseInt(text, 10);
        cy.get('[data-testid="map-zoom-in"]').click();
        cy.get('[data-testid="map-zoom-display"]')
          .invoke('text')
          .then((newText) => {
            const next = parseInt(newText, 10);
            expect(next > initial).to.equal(true);
          });
      });
  });

  it('resets zoom to 100%', () => {
    cy.get('[data-testid="map-zoom-in"]').click();
    cy.get('[data-testid="map-zoom-reset"]').click();
    cy.get('[data-testid="map-zoom-display"]').should('contain', '100%');
  });

  it('shows help dialog with controls heading', () => {
    cy.get('[data-testid="map-help"]').click();
    cy.contains('h2', 'Controls').should('be.visible');
    cy.get('[data-testid="modal-close-button"]').click();
  });
});
