import { defineStore } from "pinia";
import * as _ from "lodash-es";
import { watch } from "vue";
import { addStoreHooks, ApiRequest } from "@si/vue-lib/pinia";
import storage from "local-storage-fallback";
import { useRealtimeStore } from "@/store/realtime/realtime.store";
import { ModuleId } from "@/store/module.store";
import router from "@/router";
import { useAuthStore, UserId } from "./auth.store";
import { useRouterStore } from "./router.store";
import { AuthApiRequest } from ".";

export type WorkspacePk = string;

type WorkspaceExportId = string;
type WorkspaceExportSummary = {
  id: WorkspaceExportId;
  createdAt: IsoDateString;
};

type AuthApiWorkspace = {
  creatorUserId: string;
  displayName: string;
  id: WorkspacePk;
  pk: WorkspacePk; // not actually in the response, but we backfill
  // instanceEnvType: "LOCAL" // not used yet...
  instanceUrl: string;
  role: "OWNER" | "EDITOR";
};

export type WorkspaceImportSummary = {
  importRequestedByUserPk: UserId;
  workspaceExportCreatedAt: IsoDateString;
  workspaceExportCreatedBy: string;
  importedWorkspaceName: string;
};

const LOCAL_STORAGE_LAST_WORKSPACE_PK = "si-last-workspace-pk";

// Note(victor): The workspace import exists outside a changeset context
// (since change sets exists inside tenancies) - So no endpoints in this store
// should use a visibility. If one seems like it should, then it belongs
// in a different store.
export const useWorkspacesStore = () => {
  return addStoreHooks(
    defineStore("workspaces", {
      state: () => ({
        workspacesByPk: {} as Record<WorkspacePk, AuthApiWorkspace>,
        workspaceExports: [] as WorkspaceExportSummary[],
        workspaceImportSummary: null as WorkspaceImportSummary | null,
        workspaceApprovals: {} as Record<UserId, string>,
        importCompletedAt: null as IsoDateString | null,
        importCancelledAt: null as IsoDateString | null,
        importId: null as string | null,
        importLoading: false as boolean,
        importError: undefined as string | undefined,
      }),
      getters: {
        allWorkspaces: (state) => _.values(state.workspacesByPk),
        selectedWorkspacePk(): WorkspacePk | null {
          const pk = this.selectedWorkspace?.pk || null;
          if (pk) storage.setItem(LOCAL_STORAGE_LAST_WORKSPACE_PK, pk);
          return pk;
        },
        urlSelectedWorkspaceId: () => {
          const route = useRouterStore().currentRoute;
          return route?.params?.workspacePk as WorkspacePk | undefined;
        },
        selectedWorkspace(): AuthApiWorkspace | null {
          return _.get(
            this.workspacesByPk,
            this.urlSelectedWorkspaceId || "",
            null,
          );
        },
      },

      actions: {
        getAutoSelectedWorkspacePk() {
          const lastSelected = storage.getItem(LOCAL_STORAGE_LAST_WORKSPACE_PK);
          // here we can inject extra logic for auto selection...
          return lastSelected || this.allWorkspaces[0]?.pk;
        },

        async FETCH_USER_WORKSPACES() {
          return new AuthApiRequest<AuthApiWorkspace[]>({
            url: "/workspaces",
            onSuccess: (response) => {
              const renameIdList = _.map(response, (w) => ({
                ...w,
                pk: w.id,
              }));
              this.workspacesByPk = _.keyBy(renameIdList, "pk");

              // NOTE - we could cache this stuff in localstorage too to avoid showing loading state
              // but this is a small optimization to make later...
            },
          });
        },
        async INVITE_USER(email: string) {
          return new AuthApiRequest<void>({
            method: "post",
            url: "workspace/invite",
            params: {
              email,
            },
          });
        },
        async BEGIN_WORKSPACE_IMPORT(moduleId: ModuleId) {
          this.workspaceApprovals = {};
          this.importId = null;
          this.importLoading = true;
          this.importError = undefined;
          return new ApiRequest<{ id: string }>({
            method: "post",
            url: "/module/install_workspace",
            params: {
              id: moduleId,
            },
            onSuccess: (data) => {
              this.workspaceImportSummary = null;
              this.importId = data.id;
            },
            onFail: () => {
              this.importId = null;
              this.importLoading = false;
            },
          });
        },
        async BEGIN_APPROVAL_PROCESS(moduleId: ModuleId) {
          return new ApiRequest({
            method: "post",
            url: "/module/begin_approval_process",
            params: {
              id: moduleId,
            },
          });
        },
        async CANCEL_APPROVAL_PROCESS() {
          this.workspaceImportSummary = null;
          return new ApiRequest({
            method: "post",
            url: "/module/cancel_approval_process",
            params: {},
            onSuccess: (_response) => {
              this.workspaceImportSummary = null;
            },
          });
        },
        async IMPORT_WORKSPACE_VOTE(vote: string) {
          return new ApiRequest({
            method: "post",
            url: "/module/import_workspace_vote",
            params: {
              vote,
            },
          });
        },
      },

      onActivated() {
        const authStore = useAuthStore();
        watch(
          () => authStore.userIsLoggedInAndInitialized,
          (loggedIn) => {
            if (loggedIn) this.FETCH_USER_WORKSPACES();
          },
          { immediate: true },
        );

        const realtimeStore = useRealtimeStore();

        // Since there is only one workspace store instance,
        // we need to resubscribe when the workspace pk changes
        watch(
          () => this.selectedWorkspacePk,
          () => {
            realtimeStore.subscribe(
              this.$id,
              `workspace/${this.selectedWorkspacePk}`,
              [
                {
                  eventType: "WorkspaceImportBeginApprovalProcess",
                  callback: (data) => {
                    this.importCancelledAt = null;
                    this.importCompletedAt = null;
                    this.workspaceImportSummary = {
                      importRequestedByUserPk: data.userPk,
                      workspaceExportCreatedAt: data.createdAt,
                      workspaceExportCreatedBy: data.createdBy,
                      importedWorkspaceName: data.name,
                    };
                  },
                },
                {
                  eventType: "WorkspaceImportCancelApprovalProcess",
                  callback: () => {
                    this.workspaceApprovals = {};
                    this.workspaceImportSummary = null;
                    this.importCancelledAt = new Date().toISOString();
                    this.importCompletedAt = null;
                  },
                },
                {
                  eventType: "ImportWorkspaceVote",
                  callback: (data) => {
                    if (this.selectedWorkspacePk === data.workspacePk) {
                      this.workspaceApprovals[data.userPk] = data.vote;
                    }
                  },
                },
                {
                  eventType: "WorkspaceImported",
                  callback: () => {
                    this.workspaceApprovals = {};
                    this.workspaceImportSummary = null;
                    this.importCompletedAt = new Date().toISOString();
                    this.importCancelledAt = null;
                  },
                },
                {
                  eventType: "AsyncFinish",
                  callback: ({ id }: { id: string }) => {
                    if (id === this.importId) {
                      this.importLoading = false;
                      this.importCompletedAt = new Date().toISOString();
                      this.importError = undefined;
                      this.importId = null;

                      const route = router.currentRoute.value;

                      router.push({
                        name: "workspace-compose",
                        params: {
                          ...route.params,
                          changeSetId: "head",
                        },
                        query: route.query,
                      });
                    }
                  },
                },
                {
                  eventType: "AsyncError",
                  callback: ({ id, error }: { id: string; error: string }) => {
                    if (id === this.importId) {
                      this.importLoading = false;
                      this.importError = error;
                      this.importId = null;
                    }
                  },
                },
              ],
            );
          },
          { immediate: true },
        );

        // NOTE - don't need to clean up here, since there is only one workspace
        // store, and it will always be loaded
      },
    }),
  )();
};
