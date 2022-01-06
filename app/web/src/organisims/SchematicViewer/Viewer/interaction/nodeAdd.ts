import * as PIXI from "pixi.js";

import { SceneManager } from "../scene";
import { SchematicDataManager } from "../../data";
import * as OBJ from "../obj";
import * as MODEL from "../../model";

interface Position {
  x: number;
  y: number;
}

export interface NodeAddInteractionData {
  position: {
    mouse: {
      x: number;
      y: number;
    };
  };
}

export class NodeAddManager {
  sceneManager: SceneManager;
  dataManager: SchematicDataManager;
  data?: PIXI.InteractionData | undefined;
  node?: OBJ.Node;

  constructor(sceneManager: SceneManager, dataManager: SchematicDataManager) {
    this.sceneManager = sceneManager;
    this.dataManager = dataManager;
  }

  beforeAddNode(data: PIXI.InteractionData): void {
    this.data = data;
  }

  addNode(nodeType: string): void {
    // only render the node when the mouse moves... so that it is hidden when "added"
    console.log("adding a new node: ", nodeType, this.data);

    const n = MODEL.generateNode("temporary", "my Title", "my-name", {
      x: 0,
      y: 0,
    });
    const node = new OBJ.Node(n);
    this.sceneManager.addNode(node);

    this.node = this.sceneManager.getGeo("my-name") as OBJ.Node;
    console.log("this node", this.node);

    console.log("added a node to the scene");
    // Add temporary node to the scene
  }

  drag(): void {
    if (this.data && this.node) {
      const localPosition = this.data.getLocalPosition(this.node.parent);
      const position = {
        x: localPosition.x,
        y: localPosition.y,
      };

      this.sceneManager.translateNode(this.node, position);
      this.node.render(this.sceneManager.renderer);
    }
  }

  // afterDrag(node: Node): void {
  //   const nodeUpdate: NodeUpdate = {
  //     nodeId: node.id,
  //     position: {
  //       ctxId: "aaa",
  //       x: node.x,
  //       y: node.y,
  //     },
  //   };
  //   this.dataManager.nodeUpdate$.next(nodeUpdate);
  // }

  // offset = {
  //   x: e.data.global.x - sceneGeo.worldTransform.tx,
  //   y: e.data.global.y - sceneGeo.worldTransform.ty,
  // };

  afterAddNode(): void {
    // remove temporary node
  }
}
