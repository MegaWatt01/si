[**lang-js**](../README.md) • **Docs**

***

[lang-js](../README.md) / SecretDefinitionBuilder

# Class: SecretDefinitionBuilder

Creates a secret to be used with a set of assets

## Example

```ts
const secretDefinition = new SecretDefinitionBuilder()
         .setName("DigitalOcean Token")
        .setConnectionAnnotations("Registry Token")
        .addProp(
            new PropBuilder()
            .setKind("string")
            .setName("token")
            .setWidget(
                new PropWidgetDefinitionBuilder()
                .setKind("password")
                .build()
            )
            .build()
        )
        .build();
```

## Implements

- [`ISecretDefinitionBuilder`](../interfaces/ISecretDefinitionBuilder.md)

## Constructors

### new SecretDefinitionBuilder()

> **new SecretDefinitionBuilder**(): [`SecretDefinitionBuilder`](SecretDefinitionBuilder.md)

#### Returns

[`SecretDefinitionBuilder`](SecretDefinitionBuilder.md)

#### Defined in

[asset\_builder.ts:945](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L945)

## Properties

### definition

> **definition**: [`SecretDefinition`](../interfaces/SecretDefinition.md)

#### Defined in

[asset\_builder.ts:943](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L943)

## Methods

### setName()

> **setName**(`name`): `this`

The secret name. This corresponds to the kind of secret

#### Parameters

• **name**: `string`

the name of the secret kind

#### Returns

`this`

this

#### Example

```ts
.setName("DigitalOcean Token")
```

#### Implementation of

[`ISecretDefinitionBuilder`](../interfaces/ISecretDefinitionBuilder.md).[`setName`](../interfaces/ISecretDefinitionBuilder.md#setname)

#### Defined in

[asset\_builder.ts:962](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L962)

***

### addProp()

> **addProp**(`prop`): `this`

Adds a Prop to the secret definition. These define the form fields for the secret input

#### Parameters

• **prop**: [`PropDefinition`](../interfaces/PropDefinition.md)

{PropDefinition}

#### Returns

`this`

this

#### Example

```ts
.addProp(new PropBuilder()
    .setName("token")
    .setKind("string")
    .setWidget(new PropWidgetDefinitionBuilder().setKind("password").build())
    .build())
```

#### Implementation of

[`ISecretDefinitionBuilder`](../interfaces/ISecretDefinitionBuilder.md).[`addProp`](../interfaces/ISecretDefinitionBuilder.md#addprop)

#### Defined in

[asset\_builder.ts:981](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L981)

***

### setConnectionAnnotation()

> **setConnectionAnnotation**(`connectionAnnotations`): `this`

Adds the specified connection annotations to the output socket for the secret

#### Parameters

• **connectionAnnotations**: `string`

the connection annotations to create for the output socket.

#### Returns

`this`

this

#### Example

```ts
.setConnectionAnnotation("Registry Token")
```

#### Defined in

[asset\_builder.ts:996](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L996)

***

### build()

> **build**(): [`SecretDefinition`](../interfaces/SecretDefinition.md)

#### Returns

[`SecretDefinition`](../interfaces/SecretDefinition.md)

#### Implementation of

[`ISecretDefinitionBuilder`](../interfaces/ISecretDefinitionBuilder.md).[`build`](../interfaces/ISecretDefinitionBuilder.md#build)

#### Defined in

[asset\_builder.ts:1001](https://github.com/systeminit/si/blob/main/bin/lang-js/src/asset_builder.ts#L1001)