<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            class Concrete extends AbstractThing { public function doIt(): void {} }
            function test(): Concrete { return new Concrete(); }
        