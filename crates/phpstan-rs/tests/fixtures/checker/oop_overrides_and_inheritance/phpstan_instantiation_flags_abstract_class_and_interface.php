<?php
            abstract class LoremInstantiation {}
            interface IpsumInstantiation {}
            function test(): void {
                new LoremInstantiation();
                new IpsumInstantiation();
            }
        