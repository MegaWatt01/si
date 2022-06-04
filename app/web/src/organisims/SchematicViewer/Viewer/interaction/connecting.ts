import * as PIXI from "pixi.js";
import * as OBJ from "../obj";
import _ from "lodash";

import { SceneManager } from "../scene";
import { SchematicDataManager } from "../../data";
import {
  variantById,
  inputSocketByVariantAndProvider,
  inputSocketById,
  outputSocketById,
} from "@/api/sdf/dal/schematic";

interface Position {
  x: number;
  y: number;
}

export interface ConnectingInteractionData {
  position: {
    mouse: {
      x: number;
      y: number;
    };
  };
}

export class ConnectingManager {
  dataManager: SchematicDataManager;
  zoomFactor: number;
  interactiveConnection?: undefined;
  sourceSocket?: string | undefined;
  destinationSocket?: string | undefined;
  connection?: OBJ.Connection | undefined | null;
  data?: PIXI.InteractionData | undefined;
  offset?: Position | undefined;

  constructor(dataManager: SchematicDataManager) {
    this.dataManager = dataManager;
    this.zoomFactor = 1;
  }

  async beforeConnect(
    data: PIXI.InteractionData,
    target: OBJ.Connection,
    sceneManager: SceneManager,
    offset: Position,
    zoomFactor: number,
  ): Promise<void> {
    this.zoomFactor = zoomFactor;
    this.data = data;
    this.sourceSocket = target.name;
    this.offset = {
      x: offset.x,
      y: offset.y,
    };

    //  (sp.x - offset.x) * (1 / this.zoomFactor),

    const sourceSocketId = parseInt(this.sourceSocket.split(".")[1]);
    const sourceSocket = await outputSocketById(sourceSocketId);
    const metadata = sourceSocket.provider.ty;

    const nodes = sceneManager.group?.nodes?.children as OBJ.Node[] | undefined;
    for (const node of nodes ?? []) {
      const variant = await variantById(node.schemaVariantId);
      try {
        inputSocketByVariantAndProvider(variant, metadata);
      } catch {
        node.dim();
      }
    }

    sceneManager.interactiveConnection = sceneManager.createConnection(
      {
        x: (target.worldTransform.tx - offset.x) * (1 / this.zoomFactor),
        y: (target.worldTransform.ty - offset.y) * (1 / this.zoomFactor),
      },
      {
        x: (data.global.x - offset.x) * (1 / this.zoomFactor),
        y: (data.global.y - offset.y) * (1 / this.zoomFactor),
      },
      sourceSocket.name,
      destinationSocket.name,
      true,
    );
  }

  drag(data: PIXI.InteractionData, sceneManager: SceneManager): void {
    if (sceneManager.interactiveConnection && this.offset) {
      sceneManager.updateConnectionInteractive(
        sceneManager.interactiveConnection.name,
        {
          x: (data.global.x - this.offset.x) * (1 / this.zoomFactor),
          y: (data.global.y - this.offset.y) * (1 / this.zoomFactor),
        },
      );
      sceneManager.refreshConnections();
    }
  }

  connect(socket: string): void {
    this.destinationSocket = socket;
  }

  async afterConnect(sceneManager: SceneManager): Promise<void> {
    const nodes = sceneManager.group?.nodes?.children as OBJ.Node[] | undefined;
    for (const node of nodes ?? []) {
      node.undim();
    }

    if (this.sourceSocket && this.destinationSocket && this.offset) {
      const source = sceneManager.getGeo(this.sourceSocket);
      const destination = sceneManager.getGeo(this.destinationSocket);

      const sourceSocketStr = source.name.split(".");
      const sourceNodeId = parseInt(sourceSocketStr[0]);
      const sourceSocketId = parseInt(sourceSocketStr[1]);

      const destinationSocketStr = destination.name.split(".");
      const destinationNodeId = parseInt(destinationSocketStr[0]);
      const destinationSocketId = parseInt(destinationSocketStr[1]);

      const sourceSocket = await outputSocketById(sourceSocketId);
      const destinationSocket = await inputSocketById(destinationSocketId);

      if (_.isEqual(sourceSocket.provider.ty, destinationSocket.provider.ty)) {
        sceneManager.createConnection(
          {
            x:
              (source.worldTransform.tx - this.offset.x) *
              (1 / this.zoomFactor),
            y:
              (source.worldTransform.ty - this.offset.y) *
              (1 / this.zoomFactor),
          },
          {
            x:
              (destination.worldTransform.tx - this.offset.x) *
              (1 / this.zoomFactor),
            y:
              (destination.worldTransform.ty - this.offset.y) *
              (1 / this.zoomFactor),
          },
          source.name,
          destination.name,
        );
        this.clearInteractiveConnection(sceneManager);
        sceneManager.refreshConnections();
        this.dataManager.createConnection({
          sourceNodeId,
          sourceSocketId,
          destinationNodeId,
          destinationSocketId,
        });
      }
    }
  }

  clearInteractiveConnection(sceneManager: SceneManager): void {
    if (sceneManager.interactiveConnection) {
      sceneManager.removeConnection(sceneManager.interactiveConnection.name);
    }
  }
}
