<?php
            namespace Vendor\Internal;
            /** @internal */
            interface IBase {}

            namespace Other\App;
            enum E implements \Vendor\Internal\IBase {
                case A;
            }
        