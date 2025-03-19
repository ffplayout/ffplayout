export const useVariables = () => {
    const multiSelectClasses = {
        container: 'relative bg-base-100 border border-base-content/20 w-full h-auto flex items-center justify-end px-0 min-h-[32px] rounded',
        containerDisabled: '[&>div]:cursor-default !bg-base-100 [&>div>div]:pr-2',
        containerOpen: 'rounded-b-none',
        containerOpenTop: 'rounded-t-none',
        containerActive: 'ring-3 ring-primary',
        wrapper: 'relative mx-auto w-full flex items-center justify-end box-border cursor-pointer outline-hidden',
        singleLabel:
            'flex items-center h-full max-w-full absolute left-0 top-0 pointer-events-none bg-transparent leading-snug pl-3.5 pr-16 box-border rtl:left-auto rtl:right-0 rtl:pl-0 rtl:pr-3.5',
        singleLabelText: 'overflow-ellipsis overflow-hidden block whitespace-nowrap max-w-full',
        multipleLabel:
            'flex items-center h-full absolute left-0 top-0 pointer-events-none bg-transparent leading-snug pl-3.5 rtl:left-auto rtl:right-0 rtl:pl-0 rtl:pr-3.5',
        search: 'w-full absolute inset-0 outline-hidden focus:ring-0 appearance-none box-border border-0 text-base font-sans bg-base-100 rounded-sm pl-3.5 rtl:pl-0 rtl:pr-3.5',
        tags: 'grow shrink flex flex-wrap items-center mt-0 pl-2 min-w-0 rtl:pl-0 rtl:pr-2',
        tag: 'bg-base-300 h-6 text-base-content text-sm font-bold py-0.5 pl-2 rounded-sm mr-1 my-0.5 flex items-center whitespace-nowrap min-w-0 rtl:pl-0 rtl:pr-2 rtl:mr-0 rtl:ml-1',
        tagDisabled: 'pr-2 opacity-50 rtl:pl-2',
        tagWrapper: 'whitespace-nowrap overflow-hidden overflow-ellipsis',
        tagWrapperBreak: 'whitespace-normal break-all',
        tagRemove: 'flex items-center justify-center p-1 mx-0.5 rounded-xs hover:bg-black/10 group',
        tagRemoveIcon:
            'bg-multiselect-remove bg-center bg-no-repeat opacity-30 inline-block w-3 h-3 group-hover:opacity-60',
        tagsSearchWrapper: 'inline-block relative mx-1 grow shrink h-[24px]',
        tagsSearch:
            'absolute inset-0 border-0 outline-hidden focus:ring-0 appearance-none p-0 text-base font-sans box-border w-full h-full',
        tagsSearchCopy: 'invisible whitespace-pre-wrap inline-block h-px',
        placeholder:
            'flex items-center h-full absolute left-0 top-0 pointer-events-none bg-transparent leading-snug pl-3.5 text-gray-400 rtl:left-auto rtl:right-0 rtl:pl-0 rtl:pr-3.5',
        caret: 'multiselect-caret bg-center bg-no-repeat w-2.5 h-4 py-px box-content mr-3.5 relative z-10 shrink-0 grow-0 transition-transform transform pointer-events-none rtl:mr-0 rtl:ml-3.5',
        caretOpen: 'rotate-180 pointer-events-auto',
        clear: 'pr-3.5 relative z-10 transition duration-300 shrink-0 grow-0 flex hover:opacity-80 rtl:pr-0 rtl:pl-3.5',
        clearIcon: 'multiselect-clear-icon bg-center bg-no-repeat w-2.5 h-4 py-px box-content inline-block',
        spinner:
            'bg-multiselect-spinner bg-center bg-no-repeat w-4 h-4 z-10 mr-3.5 animate-spin shrink-0 grow-0 rtl:mr-0 rtl:ml-3.5',
        infinite: 'flex items-center justify-center w-full',
        infiniteSpinner:
            'bg-multiselect-spinner bg-center bg-no-repeat w-4 h-4 z-10 animate-spin shrink-0 grow-0 m-3.5',
        dropdown:
            'max-h-60 absolute -left-px -right-px bottom-0 transform translate-y-full border border-base-content/50 -mt-px overflow-y-scroll z-50 bg-base-100 flex flex-col rounded-b',
        dropdownTop: '-translate-y-full top-px bottom-auto rounded-b-none rounded-t',
        dropdownHidden: 'hidden',
        options: 'flex flex-col p-0 m-0 list-none',
        optionsTop: '',
        group: 'p-0 m-0',
        groupLabel:
            'flex text-sm box-border items-center justify-start text-left py-1 px-3 font-bold bg-gray-200 cursor-default leading-normal',
        groupLabelPointable: 'cursor-pointer',
        groupLabelPointed: 'bg-gray-300 text-gray-700',
        groupLabelSelected: 'bg-accent text-my-text',
        groupLabelDisabled: 'bg-gray-100 text-gray-300 cursor-not-allowed',
        groupLabelSelectedPointed: 'bg-accent text-my-text opacity-90',
        groupLabelSelectedDisabled: 'text-green-100 bg-green-600/50 cursor-not-allowed',
        groupOptions: 'p-0 m-0',
        option: 'flex items-center justify-start box-border text-left cursor-pointer text-base leading-snug py-2 px-3',
        optionPointed: 'text-gray-800 bg-secondary',
        optionSelected: 'text-my-text bg-accent',
        optionDisabled: 'text-gray-300 cursor-not-allowed',
        optionSelectedPointed: 'text-my-text bg-link-hover',
        optionSelectedDisabled: 'text-green-100 bg-green-500/50 cursor-not-allowed',
        noOptions: 'py-2 px-3 text-gray-600 bg-base-100 text-left rtl:text-right',
        noResults: 'py-2 px-3 text-gray-600 bg-base-100 text-left rtl:text-right',
        fakeInput:
            'bg-transparent absolute left-0 right-0 -bottom-px w-full h-px border-0 p-0 appearance-none outline-hidden text-transparent',
        assist: 'absolute -m-px w-px h-px overflow-hidden',
        spacer: 'h-6 py-px box-content',
    }
    return {
        multiSelectClasses,
    }
}
