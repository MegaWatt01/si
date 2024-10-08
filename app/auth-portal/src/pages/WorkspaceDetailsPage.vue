<template>
  <div class="overflow-hidden">
    <template v-if="loadWorkspacesReqStatus.isSuccess || createMode">
      <div
        class="flex flex-row gap-sm align-middle items-center justify-between"
      >
        <div
          ref="workspaceNameRef"
          v-tooltip="workspaceNameTooltip"
          class="text-lg font-bold line-clamp-3 break-words"
        >
          {{
            draftWorkspace.displayName ||
            (createMode ? "Create New Workspace" : "Workspace Details")
          }}
        </div>
        <RouterLink
          :to="{
            name: 'workspaces',
          }"
        >
          <VButton label="Return To Workspaces" tone="neutral" />
        </RouterLink>
      </div>
      <div class="mt-sm pb-md">
        <div>
          {{
            createMode
              ? "Fill out this form to create a new workspace."
              : "From here you can manage this workspace and invite users to be part of it."
          }}
        </div>
      </div>

      <Stack>
        <ErrorMessage
          :requestStatus="
            createMode ? createWorkspaceReqStatus : editWorkspaceReqStatus
          "
        />
        <VormInput
          v-model="draftWorkspace.displayName"
          label="Display Name"
          placeholder="A display name for this workspace"
          required
          :disabled="!isWorkspaceOwner && !createMode"
          :maxLength="500"
        />
        <VormInput
          v-if="createMode"
          v-model="createWorkspaceType"
          :options="createWorkspaceTypeDropdownOptions"
          required
          label="Workspace Type"
          placeholder="Choose what kind of workspace to create"
          type="dropdown"
        />

        <VormInput
          v-if="createWorkspaceType === 'url'"
          ref="urlInputRef"
          v-model="draftWorkspace.instanceUrl"
          label="URL"
          autocomplete="url"
          placeholder="The instance url for this workspace"
          required
          :disabled="!isWorkspaceOwner && !createMode"
        />

        <VButton
          v-if="!createMode || createWorkspaceType"
          iconRight="chevron--right"
          :disabled="
            validationState.isError || (!isWorkspaceOwner && !createMode)
          "
          :requestStatus="
            createMode ? createWorkspaceReqStatus : editWorkspaceReqStatus
          "
          :loadingText="createMode ? 'Creating...' : 'Applying...'"
          tone="action"
          variant="solid"
          @click="() => (createMode ? createWorkspace() : editWorkspace())"
        >
          {{ createMode ? "Create Workspace" : "Save" }}
        </VButton>
      </Stack>
      <div v-if="!createMode" class="mt-sm">
        <template v-if="loadWorkspaceMembersReqStatus.isPending">
          <Icon name="loader" />
        </template>
        <template v-else-if="loadWorkspaceMembersReqStatus.isError">
          <ErrorMessage :requestStatus="loadWorkspaceMembersReqStatus" />
        </template>
        <template v-else-if="loadWorkspaceMembersReqStatus.isSuccess">
          <div class="relative">
            <Stack>
              <div class="text-lg font-bold">Members of this workspace:</div>

              <table
                class="w-full divide-y divide-neutral-400 dark:divide-neutral-600 border-b border-neutral-400 dark:border-neutral-600"
              >
                <thead>
                  <tr
                    class="children:pb-xs children:px-md children:font-bold text-left text-xs uppercase"
                  >
                    <th scope="col">Email</th>
                    <th scope="col">Role</th>
                    <th scope="col">Remove User</th>
                    <th scope="col">Change Workspace Role</th>
                  </tr>
                </thead>
                <tbody
                  class="divide-y divide-neutral-300 dark:divide-neutral-700"
                >
                  <tr
                    v-for="memUser in members"
                    :key="memUser.userId"
                    class="children:px-md children:py-sm children:truncate text-sm font-medium text-gray-800 dark:text-gray-200"
                  >
                    <td class="">
                      <div
                        class="xl:max-w-[800px] lg:max-w-[60vw] md:max-w-[50vw] sm:max-w-[40vw] max-w-[150px] truncate"
                      >
                        {{ memUser.email }}
                      </div>
                    </td>
                    <td class="normal-case">
                      {{ memUser.role }}
                    </td>
                    <td class="normal-case">
                      <ErrorMessage :requestStatus="deleteUserHandlerReq" />
                      <div
                        v-if="memUser.role !== 'OWNER'"
                        class="cursor-pointer hover:text-destructive-500"
                        @click="deleteUserHandler(memUser.email)"
                      >
                        <Icon name="trash" />
                      </div>
                    </td>
                    <td class="normal-case">
                      <ErrorMessage :requestStatus="changeMembershipReq" />
                      <VButton
                        v-if="memUser.role === 'OWNER'"
                        :disabled="hasOnlyOneOwner"
                        label="Make Editor"
                        tone="action"
                        variant="solid"
                        @click="changeMembership(memUser.userId, 'EDITOR')"
                      />
                      <VButton
                        v-else
                        label="Make Owner"
                        tone="action"
                        variant="solid"
                        @click="changeMembership(memUser.userId, 'OWNER')"
                      />
                    </td>
                  </tr>
                </tbody>
              </table>
            </Stack>
          </div>
        </template>
      </div>
      <div
        v-if="
          inviteUserReqStatus.isPending ||
          inviteUserReqStatus.isError ||
          !createMode
        "
        class="pt-md"
      >
        <template v-if="inviteUserReqStatus.isPending">
          <Icon name="loader" />
        </template>
        <template v-else-if="inviteUserReqStatus.isError">
          <ErrorMessage :requestStatus="inviteUserReqStatus" />
        </template>
        <template v-if="!createMode">
          <Stack spacing="lg">
            <Stack>
              <VormInput
                v-model="newMember.email"
                type="email"
                label="User Email to Grant Workspace Access"
                @enterPressed="inviteButtonHandler"
              />
              <VButton
                class="flex-none"
                tone="action"
                variant="solid"
                :requestStatus="inviteUserReqStatus"
                @click="inviteButtonHandler"
                >Add User To Workspace</VButton
              >
            </Stack>
          </Stack>
        </template>
      </div>
      <div class="flex justify-between items-center">
        <div
          v-if="
            featureFlagsStore.DELETE_WORKSPACE &&
            isWorkspaceOwner &&
            !createMode
          "
          class="pt-md"
        >
          <VButton
            iconRight="chevron--right"
            :disabled="!isWorkspaceOwner"
            :requestStatus="deleteWorkspaceReqStatus"
            loadingText="Deleting..."
            tone="action"
            variant="solid"
            @click="() => deleteWorkspace()"
          >
            Delete Workspace
          </VButton>
        </div>
        <div class="pt-md">
          <VButton
            label="Go to workspace"
            tone="action"
            variant="solid"
            @click="() => launchWorkspace()"
          >
          </VButton>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import * as _ from "lodash-es";
import { computed, PropType, reactive, ref, watch } from "vue";
import {
  Icon,
  VormInput,
  Stack,
  ErrorMessage,
  VButton,
  useValidatedInputGroup,
} from "@si/vue-lib/design-system";
import { useHead } from "@vueuse/head";
import { useRouter } from "vue-router";
import { useAuthStore } from "@/store/auth.store";
import { useWorkspacesStore, WorkspaceId } from "@/store/workspaces.store";
import { useFeatureFlagsStore } from "@/store/feature_flags.store";
import { tracker } from "@/lib/posthog";
import { API_HTTP_URL } from "@/store/api";

const authStore = useAuthStore();
const workspacesStore = useWorkspacesStore();
const router = useRouter();
const featureFlagsStore = useFeatureFlagsStore();

const props = defineProps({
  workspaceId: { type: String as PropType<WorkspaceId>, required: true },
});

const urlInputRef = ref<InstanceType<typeof VormInput>>();

const { validationState, validationMethods } = useValidatedInputGroup();

const members = computed(() => workspacesStore.selectedWorkspaceMembers);
const blankWorkspace = {
  instanceUrl: "",
  displayName: "",
  isDefault: false,
};
const draftWorkspace = reactive(_.cloneDeep(blankWorkspace));
const newMember = reactive({ email: "", role: "editor" });
useHead({ title: "Workspace Details" });

const createWorkspaceReqStatus =
  workspacesStore.getRequestStatus("CREATE_WORKSPACE");
const editWorkspaceReqStatus =
  workspacesStore.getRequestStatus("EDIT_WORKSPACE");
const loadWorkspaceMembersReqStatus = workspacesStore.getRequestStatus(
  "LOAD_WORKSPACE_MEMBERS",
);
const inviteUserReqStatus = workspacesStore.getRequestStatus(
  "INVITE_USER",
  "email",
);
const deleteWorkspaceReqStatus =
  workspacesStore.getRequestStatus("DELETE_WORKSPACE");

const hasOnlyOneOwner = computed(() => {
  const ownerCount = members.value.filter(
    (member) => member.role === "OWNER",
  ).length;
  return ownerCount === 1;
});

const createMode = computed(() => props.workspaceId === "new");
const isWorkspaceOwner = computed(
  () =>
    props.workspaceId === "new" ||
    workspacesStore.workspacesById[props.workspaceId]?.role === "OWNER",
);

const loadWorkspacesReqStatus =
  workspacesStore.getRequestStatus("LOAD_WORKSPACES");

function reloadWorkspaces() {
  if (import.meta.env.SSR) return;
  if (!authStore.userIsLoggedIn) return;

  // eslint-disable-next-line @typescript-eslint/no-floating-promises
  workspacesStore.LOAD_WORKSPACES();
}
watch(() => authStore.userIsLoggedIn, reloadWorkspaces, { immediate: true });

watch(
  [() => props.workspaceId, loadWorkspacesReqStatus],
  () => {
    if (!loadWorkspacesReqStatus.value.isSuccess) return;
    _.assign(
      draftWorkspace,
      _.cloneDeep(
        createMode.value
          ? blankWorkspace
          : workspacesStore.workspacesById[props.workspaceId],
      ),
    );
    if (!createMode.value) {
      // eslint-disable-next-line @typescript-eslint/no-floating-promises
      workspacesStore.LOAD_WORKSPACE_MEMBERS(props.workspaceId);
    }
  },
  { immediate: true },
);
const createWorkspace = async () => {
  if (createWorkspaceType.value === "saas") {
    draftWorkspace.instanceUrl = "https://app.systeminit.com";
  } else if (createWorkspaceType.value === "local") {
    draftWorkspace.instanceUrl = "http://localhost:8080";
  } else {
    if (draftWorkspace.instanceUrl.includes("app.systeminit")) {
      // Can't create a Remote URL workspace with our URL!
      urlInputRef.value?.setError(
        'You cannot use an "app.systeminit" URL for a Remote URL Workspace. Use "Managed By System Initiative" instead.',
      );
      return;
    } else if (
      draftWorkspace.instanceUrl.includes("localhost") ||
      draftWorkspace.instanceUrl.includes("127.0.0.1")
    ) {
      // Can't create a Remote URL workspace with localhost!
      urlInputRef.value?.setError(
        'You cannot use a "localhost" URL for a Remote URL Workspace. Use "Local Dev Instance" instead.',
      );
      return;
    }
  }

  if (validationMethods.hasError()) return;

  const res = await workspacesStore.CREATE_WORKSPACE(draftWorkspace);

  if (res.result.success) {
    // eslint-disable-next-line @typescript-eslint/no-floating-promises
    await router.push({
      name: "workspace-settings",
      params: { workspaceId: res.result.data.newWorkspaceId },
    });
  }
};
const editWorkspace = async () => {
  if (validationMethods.hasError()) return;

  const res = await workspacesStore.EDIT_WORKSPACE(draftWorkspace);

  if (res.result.success) {
    // TODO(Wendy) - do we want to send users back to the workspaces when they save their edits?
    // setTimeout(async () => {
    //   await router.push({
    //     name: "workspaces",
    //   });
    // }, 500);
    return;
  }
};

const deleteWorkspace = async () => {
  const res = await workspacesStore.DELETE_WORKSPACE(props.workspaceId);
  if (res.result.success) {
    // eslint-disable-next-line @typescript-eslint/no-floating-promises
    await router.push({
      name: "workspaces",
      params: {},
    });
  }
};

const launchWorkspace = async () => {
  if (props.workspaceId) {
    tracker.trackEvent("workspace_launcher_widget_click");
    window.location.href = `${API_HTTP_URL}/workspaces/${props.workspaceId}/go`;
  }
};

const deleteUserHandlerReq = workspacesStore.getRequestStatus("REMOVE_USER");
const deleteUserHandler = async (email: string) => {
  if (email === "") return;
  const res = await workspacesStore.REMOVE_USER(email, props.workspaceId);
  if (res.result.success) {
    if (!draftWorkspace.instanceUrl.includes("localhost")) {
      window.location.href = ` ${draftWorkspace.instanceUrl}/refresh-auth?workspaceId=${props.workspaceId}`;
    }
  }
};

const changeMembershipReq =
  workspacesStore.getRequestStatus("CHANGE_MEMBERSHIP");
const changeMembership = async (userId: string, role: string) => {
  if (userId === "" || role === "") return;
  await workspacesStore.CHANGE_MEMBERSHIP(props.workspaceId, userId, role);

  await workspacesStore.LOAD_WORKSPACE_MEMBERS(props.workspaceId);
};

const inviteButtonHandler = async () => {
  if (newMember.email === "") return;
  const res = await workspacesStore.INVITE_USER(newMember, props.workspaceId);

  if (res.result.success) {
    newMember.email = "";
  }
};

const workspaceNameRef = ref();
const workspaceNameTooltip = computed(() => {
  if (
    workspaceNameRef.value &&
    workspaceNameRef.value.scrollHeight > workspaceNameRef.value.offsetHeight
  ) {
    return {
      content: draftWorkspace.displayName,
      delay: { show: 700, hide: 10 },
    };
  } else {
    return {};
  }
});

type WorkspaceType = "saas" | "local" | "url";

const createWorkspaceType = ref<WorkspaceType | undefined>("saas");
const createWorkspaceTypeDropdownOptions = [
  { label: "Managed By System Initiative", value: "saas" },
  { label: "Local Dev Instance", value: "local" },
  { label: "Remote URL", value: "url" },
];
</script>

<style scoped>
.long-email-handler {
  max-width: 700px;
}
</style>
