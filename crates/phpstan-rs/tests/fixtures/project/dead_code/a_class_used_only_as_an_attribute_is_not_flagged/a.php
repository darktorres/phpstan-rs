<?php
#[Attribute]
class Marker {}
#[Marker]
function marked(): void {}
marked();
