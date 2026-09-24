<?php
            use function PHPStan\Testing\assertType;
            #[Attribute]
            class TemplateAttr {}
            function f(object $view): void {
                $attr = (new ReflectionClass($view))->getAttributes(TemplateAttr::class)[0]->newInstance();
                assertType('TemplateAttr', $attr);
            }
        