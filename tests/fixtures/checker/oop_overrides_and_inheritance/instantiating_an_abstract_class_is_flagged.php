<?php
            abstract class AbstractThing { abstract public function doIt(): void; }
            function test(): AbstractThing { return new AbstractThing(); }
        