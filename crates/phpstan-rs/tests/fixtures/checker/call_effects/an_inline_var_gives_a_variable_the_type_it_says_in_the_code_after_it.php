<?php
namespace CallEffects\InlineVar\Models {
    final class Holder
    {
        public static ?int $value = null;
    }

    final class Loud
    {
        public function run(): void
        {
            Holder::$value = 1;
        }
    }

    final class Quiet
    {
        public function run(): void
        {
        }
    }
}

namespace CallEffects\InlineVar {
    use CallEffects\InlineVar\Models\Holder;
    use CallEffects\InlineVar\Models\Loud;
    use CallEffects\InlineVar\Models\Quiet as Calm;

    use function PHPStan\Testing\assertType;

    function anythingAtAll(): mixed
    {
        return null;
    }

    function beforeAStatement(mixed $x): void
    {
        /** @var Calm $x */
        $x->run();
    }

    function onAnAssignment(): void
    {
        /** @var Calm $q */
        $q = anythingAtAll();
        $q->run();
    }

    function onAnAssignmentWithoutAName(): void
    {
        /** @var Calm */
        $q = anythingAtAll();
        $q->run();
    }

    function ofALoudClass(mixed $x): void
    {
        /** @var Loud $x */
        $x->run();
    }

    function writtenAfterwards(): void
    {
        /** @var Calm $q */
        $q = anythingAtAll();
        $q = anythingAtAll();
        $q->run();
    }

    function ofAClassNotKnown(mixed $x): void
    {
        /** @var Missing $x */
        $x->run();
    }

    function takenByReference(mixed &$x): void
    {
        /** @var Calm $x */
        $x->run();
    }

    /** @param list<mixed> $items */
    function inALoopBody(array $items): void
    {
        foreach ($items as $item) {
            /** @var Calm $item */
            $item->run();
        }
    }

    function theLoopCheck(): void
    {
        Holder::$value = null;
        inALoopBody([new Calm()]);
        assertType('null', Holder::$value);
    }

    function theBeforeCheck(): void
    {
        Holder::$value = null;
        beforeAStatement(new Calm());
        assertType('null', Holder::$value);
    }

    function theAssignmentCheck(): void
    {
        Holder::$value = null;
        onAnAssignment();
        assertType('null', Holder::$value);
    }

    function theUnnamedCheck(): void
    {
        Holder::$value = null;
        onAnAssignmentWithoutAName();
        assertType('null', Holder::$value);
    }

    function theLoudCheck(): void
    {
        Holder::$value = null;
        ofALoudClass(new Loud());
        assertType('int|null', Holder::$value);
    }

    function theWrittenCheck(): void
    {
        Holder::$value = null;
        writtenAfterwards();
        assertType('int|null', Holder::$value);
    }

    function theUnknownClassCheck(): void
    {
        Holder::$value = null;
        ofAClassNotKnown(new Loud());
        assertType('int|null', Holder::$value);
    }

    function theReferenceCheck(): void
    {
        Holder::$value = null;
        $x = new Calm();
        takenByReference($x);
        assertType('int|null', Holder::$value);
    }
}
