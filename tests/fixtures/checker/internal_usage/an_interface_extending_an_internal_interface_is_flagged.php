<?php
            namespace Vendor\Internal;
            /** @internal */
            interface IBase {}

            namespace Other\App;
            interface IChild extends \Vendor\Internal\IBase {}
        